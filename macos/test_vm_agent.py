#!/usr/bin/env python3
"""Local harness tests for vm-agent.py.

Runs the VM agent against a temp directory (no Tart VM needed) and verifies
the shared-directory protocol: sentinel creation, all request types, and
error handling.

Usage:
    python3 macos/test_vm_agent.py
"""
import base64
import json
import os
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path

AGENT_SCRIPT = Path(__file__).parent / "vm-agent.py"
POLL_INTERVAL = 0.1  # seconds between checks
TIMEOUT = 5  # seconds to wait for a response


def wait_for_file(path: Path, timeout: float = TIMEOUT) -> None:
    deadline = time.time() + timeout
    while not path.exists():
        if time.time() > deadline:
            raise TimeoutError(f"Timed out waiting for {path}")
        time.sleep(POLL_INTERVAL)


def send_request(shared_dir: Path, request: dict, timeout: float = TIMEOUT) -> dict:
    """Write a request file and wait for the response."""
    requests_dir = shared_dir / "requests"
    responses_dir = shared_dir / "responses"

    # Use timestamp + counter for unique IDs
    request_id = f"test_{int(time.time() * 1000)}_{os.getpid()}"
    request_path = requests_dir / f"cmd_{request_id}.json"
    response_path = responses_dir / f"cmd_{request_id}.result.json"

    request_path.write_text(json.dumps(request))
    wait_for_file(response_path, timeout)

    result = json.loads(response_path.read_text())
    return result


class AgentHarness:
    """Context manager that starts vm-agent.py against a temp dir."""

    def __init__(self):
        self.tmpdir = None
        self.shared_dir = None
        self.proc = None

    def __enter__(self):
        self.tmpdir = tempfile.TemporaryDirectory()
        self.shared_dir = Path(self.tmpdir.name)
        self.proc = subprocess.Popen(
            [sys.executable, str(AGENT_SCRIPT), str(self.shared_dir)],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        # Wait for agent_ready sentinel
        wait_for_file(self.shared_dir / "agent_ready")
        return self

    def __exit__(self, *args):
        if self.proc and self.proc.poll() is None:
            self.proc.send_signal(signal.SIGTERM)
            try:
                self.proc.wait(timeout=3)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait()
        if self.tmpdir:
            self.tmpdir.cleanup()

    def request(self, req: dict, timeout: float = TIMEOUT) -> dict:
        return send_request(self.shared_dir, req, timeout)


# ---------- Tests ----------

def test_agent_ready_sentinel():
    """Agent writes agent_ready on startup."""
    with AgentHarness() as h:
        sentinel = h.shared_dir / "agent_ready"
        assert sentinel.exists(), "agent_ready sentinel not found"
        assert sentinel.read_text().strip() == "ready"
    print("  PASS: agent_ready sentinel")


def test_exec():
    """Basic exec returns stdout."""
    with AgentHarness() as h:
        result = h.request({"type": "exec", "cmd": ["echo", "hello world"]})
        assert result["exit_code"] == 0, f"exit_code={result['exit_code']}"
        assert "hello world" in result["stdout"], f"stdout={result['stdout']!r}"
        assert result["error"] is None
        assert "duration_ms" in result
    print("  PASS: exec")


def test_exec_exit_code():
    """exec_exit_code returns nonzero exit code."""
    with AgentHarness() as h:
        result = h.request({"type": "exec_exit_code", "cmd": ["sh", "-c", "exit 42"]})
        assert result["exit_code"] == 42, f"exit_code={result['exit_code']}"
        assert result["error"] is None
    print("  PASS: exec_exit_code")


def test_exec_stdin():
    """exec_stdin pipes base64-decoded data to stdin."""
    with AgentHarness() as h:
        input_text = "hello from stdin"
        encoded = base64.b64encode(input_text.encode()).decode()
        result = h.request({
            "type": "exec_stdin",
            "cmd": ["cat"],
            "stdin_b64": encoded,
        })
        assert result["exit_code"] == 0, f"exit_code={result['exit_code']}"
        assert "hello from stdin" in result["stdout"], f"stdout={result['stdout']!r}"
    print("  PASS: exec_stdin")


def test_exec_stderr_merged():
    """exec captures stderr into stdout."""
    with AgentHarness() as h:
        result = h.request({
            "type": "exec",
            "cmd": ["sh", "-c", "echo out; echo err >&2"],
        })
        assert result["exit_code"] == 0
        assert "out" in result["stdout"]
        assert "err" in result["stdout"], "stderr should be merged into stdout"
    print("  PASS: exec stderr merged")


def test_exec_detached():
    """exec_detached launches a background process without blocking."""
    with AgentHarness() as h:
        marker = h.shared_dir / "detached_marker"
        result = h.request({
            "type": "exec_detached",
            "cmd": ["sh", "-c", f"echo done > {marker}"],
        })
        assert result["exit_code"] == 0
        assert result["error"] is None
        # Give the background process a moment to finish
        wait_for_file(marker, timeout=3)
        assert marker.read_text().strip() == "done"
    print("  PASS: exec_detached")


def test_copy_to_vm():
    """copy_to_vm stages a file from transfers/ into dest_path."""
    with AgentHarness() as h:
        # Stage a file in transfers/
        transfer_id = "test_copy_in"
        stage_dir = h.shared_dir / "transfers" / transfer_id
        stage_dir.mkdir(parents=True)
        staged_file = stage_dir / "hello.txt"
        staged_file.write_text("copied content")

        dest_dir = h.shared_dir / "dest_target"
        result = h.request({
            "type": "copy_to_vm",
            "transfer_path": f"transfers/{transfer_id}/hello.txt",
            "dest_path": str(dest_dir),
        })
        assert result["exit_code"] == 0, f"error={result.get('error')}"
        copied = dest_dir / "hello.txt"
        assert copied.exists(), f"expected {copied} to exist"
        assert copied.read_text() == "copied content"
    print("  PASS: copy_to_vm")


def test_copy_from_vm():
    """copy_from_vm copies a file from the VM into transfers/."""
    with AgentHarness() as h:
        # Create a source file to copy
        src_file = h.shared_dir / "source_file.txt"
        src_file.write_text("from vm")

        transfer_id = "test_copy_out"
        stage_dir = h.shared_dir / "transfers" / transfer_id
        stage_dir.mkdir(parents=True)

        result = h.request({
            "type": "copy_from_vm",
            "src_path": str(src_file),
            "transfer_path": f"transfers/{transfer_id}",
        })
        assert result["exit_code"] == 0, f"error={result.get('error')}"
        copied = stage_dir / "source_file.txt"
        assert copied.exists(), f"expected {copied} to exist"
        assert copied.read_text() == "from vm"
    print("  PASS: copy_from_vm")


def test_copy_from_vm_directory():
    """copy_from_vm copies an entire directory."""
    with AgentHarness() as h:
        src_dir = h.shared_dir / "source_dir"
        src_dir.mkdir()
        (src_dir / "a.txt").write_text("aaa")
        nested = src_dir / "nested"
        nested.mkdir()
        (nested / "b.txt").write_text("bbb")

        transfer_id = "test_copy_dir_out"
        stage_dir = h.shared_dir / "transfers" / transfer_id
        stage_dir.mkdir(parents=True)

        result = h.request({
            "type": "copy_from_vm",
            "src_path": str(src_dir),
            "transfer_path": f"transfers/{transfer_id}",
        })
        assert result["exit_code"] == 0, f"error={result.get('error')}"
        copied_dir = stage_dir / "source_dir"
        assert (copied_dir / "a.txt").read_text() == "aaa"
        assert (copied_dir / "nested" / "b.txt").read_text() == "bbb"
    print("  PASS: copy_from_vm directory")


def test_unknown_request_type():
    """Unknown request type returns error."""
    with AgentHarness() as h:
        result = h.request({"type": "bogus_type", "cmd": ["echo"]})
        assert result["exit_code"] == 1
        assert result["error"] is not None
        assert "Unknown request type" in result["error"]
    print("  PASS: unknown request type")


def test_copy_from_vm_nonexistent_source():
    """copy_from_vm with nonexistent source returns error."""
    with AgentHarness() as h:
        transfer_id = "test_copy_missing"
        stage_dir = h.shared_dir / "transfers" / transfer_id
        stage_dir.mkdir(parents=True)

        result = h.request({
            "type": "copy_from_vm",
            "src_path": "/nonexistent/path/file.txt",
            "transfer_path": f"transfers/{transfer_id}",
        })
        assert result["exit_code"] == 1, f"expected error but got exit_code={result['exit_code']}"
        assert result["error"] is not None
    print("  PASS: copy_from_vm nonexistent source")


def test_exec_command_not_found():
    """exec with a nonexistent command returns nonzero exit code."""
    with AgentHarness() as h:
        result = h.request({
            "type": "exec",
            "cmd": ["this_command_does_not_exist_xyz"],
        })
        # Should get an error (either via exit_code or error field)
        assert result["exit_code"] != 0 or result["error"] is not None
    print("  PASS: exec command not found")


def test_multiple_sequential_requests():
    """Multiple requests are handled in sequence."""
    with AgentHarness() as h:
        for i in range(5):
            result = h.request({"type": "exec", "cmd": ["echo", str(i)]})
            assert result["exit_code"] == 0
            assert str(i) in result["stdout"]
    print("  PASS: multiple sequential requests")


def main():
    if not AGENT_SCRIPT.exists():
        print(f"ERROR: {AGENT_SCRIPT} not found")
        return 1

    tests = [
        test_agent_ready_sentinel,
        test_exec,
        test_exec_exit_code,
        test_exec_stdin,
        test_exec_stderr_merged,
        test_exec_detached,
        test_copy_to_vm,
        test_copy_from_vm,
        test_copy_from_vm_directory,
        test_unknown_request_type,
        test_copy_from_vm_nonexistent_source,
        test_exec_command_not_found,
        test_multiple_sequential_requests,
    ]

    passed = 0
    failed = 0
    for test in tests:
        name = test.__name__
        try:
            test()
            passed += 1
        except Exception as e:
            print(f"  FAIL: {name}: {e}")
            failed += 1

    print(f"\n{passed} passed, {failed} failed out of {len(tests)} tests")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
