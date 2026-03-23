# TODO

## Security Hardening

Items identified during a security audit (2026-03-23). Prioritized by impact.

### High Priority

- **Eliminate SYS_ADMIN for AppImages**: Investigate `--appimage-extract-and-run` as default for AppImage deploys. This would remove the need for `CAP_SYS_ADMIN` + `/dev/fuse` entirely, closing the container escape chain. Fall back to FUSE only if extraction fails.

- **Container network egress restrictions**: Add an option (e.g., `--no-network` or a config field) to disable outbound network access inside containers. Especially important for CI use cases where LLM-generated code runs inside the container with full internet access.

- **Path traversal in evaluators**: `expected_path` in `file_compare` and `script_path` in `script_replay` are read directly from task JSON without validation. Canonicalize paths and verify they stay within the project/working directory before reading.

- **Path traversal in tar extraction**: `docker/transfer.rs` `copy_from()` strips the first tar component but doesn't reject `..` sequences or absolute paths in remaining components. A compromised container could write files outside the intended destination.

- **Sanitize API error responses**: `provider/http_base.rs` and `provider/anthropic.rs` include raw error bodies in error messages. These could leak sensitive info into logs or artifacts. Truncate and sanitize before logging.

- **SSRF via custom base URL**: The `api_base_url` config field accepts any URL with no validation. Add protocol enforcement (require HTTPS unless explicitly overridden) and optionally block private/link-local IP ranges.

### Medium Priority

- **Container resource limits**: Add default memory/CPU/PID limits to container creation. A runaway process (or malicious LLM-generated code) can currently consume all host resources.

- **Prompt injection awareness**: Raw accessibility tree text, bash output, and error feedback are interpolated directly into LLM prompts (`agent/context.rs`). A malicious application could embed prompt injection payloads in its UI text or command output. Consider structured delimiters or content-length limits.

- **Evaluator temp file races**: `evaluator/file_compare.rs` uses fixed filenames (`eval_actual_file`) in the artifacts directory. Use unique names (e.g., with UUIDs) to prevent corruption during concurrent suite runs.

- **ReDoS in evaluator regex**: Regex patterns from task JSON (`MatchMode::Regex`) are compiled without complexity limits. Consider adding a timeout or using `regex` crate's built-in linear-time guarantees (already the case — verify this is sufficient).

- **API key hygiene**: The `api_key` config field is a plain string in JSON files that could be accidentally committed. Consider documenting env-var-only usage as the recommended approach and adding a warning when `api_key` is found in a config file.

### Low Priority / Future Consideration

- **Process-level sandbox for execute-action.py**: The current `exec()` + restricted builtins approach has documented bypass vectors (attribute traversal, `__globals__`, module-level access). RestrictedPython or a seccomp-based sandbox would be stronger, though the container boundary remains the primary isolation.

- **VNC authentication option**: Add an optional `vnc_password` config field for users who need LAN-accessible VNC. Default is now localhost-only, but password auth would be useful for remote debugging workflows.

- **Monitor bind address override**: Add a `monitor_bind_addr` config field (defaulting to `127.0.0.1`) to match the VNC pattern. Users running desktest on remote machines currently have no way to expose the monitor dashboard to their workstation.

- **Image digest pinning**: Allow task JSON or config to specify image digests (`image@sha256:...`) for custom Docker images, enabling reproducible and tamper-resistant builds in CI.

- **TLS certificate pinning**: The HTTP client uses default TLS settings. For known API endpoints (api.openai.com, api.anthropic.com), certificate pinning would protect against MITM.
