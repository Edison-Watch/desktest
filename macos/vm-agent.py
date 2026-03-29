#!/usr/bin/env python3
import base64
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path


def copy_path(src: Path, dest: Path) -> None:
    if src.is_dir():
        if dest.exists():
            shutil.rmtree(dest)
        shutil.copytree(src, dest)
    else:
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(src, dest)


def handle_request(shared_dir: Path, request_path: Path) -> dict:
    started = time.time()
    stdout = ""
    exit_code = 0
    error = None

    try:
        payload = json.loads(request_path.read_text())
        kind = payload["type"]
        cmd = payload.get("cmd") or []
        stdin_b64 = payload.get("stdin_b64")
        src_path = payload.get("src_path")
        dest_path = payload.get("dest_path")
        transfer_path = payload.get("transfer_path")
        if kind in {"exec", "exec_exit_code", "exec_stdin"}:
            stdin_data = None
            if stdin_b64 is not None:
                stdin_data = base64.b64decode(stdin_b64)
            result = subprocess.run(
                cmd,
                input=stdin_data,
                capture_output=True,
                text=stdin_data is None,
                check=False,
            )
            if stdin_data is not None:
                stdout = result.stdout.decode("utf-8", errors="replace")
            else:
                stdout = result.stdout
            if result.stderr:
                stdout += result.stderr if isinstance(result.stderr, str) else result.stderr.decode("utf-8", errors="replace")
            exit_code = result.returncode
        elif kind == "exec_detached":
            subprocess.Popen(
                cmd,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                start_new_session=True,
            )
        elif kind == "copy_to_vm":
            staged = shared_dir / transfer_path
            destination_root = Path(dest_path)
            destination_root.mkdir(parents=True, exist_ok=True)
            copy_path(staged, destination_root / staged.name)
        elif kind == "copy_from_vm":
            source = Path(src_path)
            staged_root = shared_dir / transfer_path
            staged_root.mkdir(parents=True, exist_ok=True)
            copy_path(source, staged_root / source.name)
        else:
            error = f"Unknown request type: {kind}"
            exit_code = 1
    except Exception as exc:
        error = str(exc)
        exit_code = 1

    return {
        "stdout": stdout,
        "exit_code": exit_code,
        "error": error,
        "duration_ms": int((time.time() - started) * 1000),
    }


def main() -> int:
    shared_dir = Path(sys.argv[1] if len(sys.argv) > 1 else "/Volumes/My Shared Files/desktest")
    requests_dir = shared_dir / "requests"
    responses_dir = shared_dir / "responses"
    transfers_dir = shared_dir / "transfers"

    requests_dir.mkdir(parents=True, exist_ok=True)
    responses_dir.mkdir(parents=True, exist_ok=True)
    transfers_dir.mkdir(parents=True, exist_ok=True)
    (shared_dir / "agent_ready").write_text("ready\n")

    while True:
        for request_path in sorted(requests_dir.glob("cmd_*.json")):
            result = handle_request(shared_dir, request_path)
            response_path = responses_dir / request_path.name.replace(".json", ".result.json")
            tmp_path = response_path.with_suffix(".tmp")
            tmp_path.write_text(json.dumps(result))
            tmp_path.rename(response_path)
            try:
                request_path.unlink()
            except FileNotFoundError:
                pass
        time.sleep(0.2)


if __name__ == "__main__":
    raise SystemExit(main())
