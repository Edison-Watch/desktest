# TODO

## Security Hardening

Items identified during a security audit (2026-03-23). Prioritized by impact.

### High Priority

- ~~**`needs_fuse` config escape hatch**~~: Done — `needs_fuse: true` field added to `DockerImage` app config. When set, grants `CAP_SYS_ADMIN` and `/dev/fuse` to the container.

- ~~**Container network egress restrictions**~~: Done — `--no-network` global CLI flag sets Docker network mode to `"none"`, disabling all container networking.

- ~~**Sanitize API error responses**~~: Done — `sanitize_error_body()` helper truncates error bodies to 500 chars in both `http_base.rs` and `anthropic.rs`.

- ~~**SSRF via custom base URL**~~: Done — `api_base_url` is now validated in `Config::validate()`: requires HTTPS (except localhost), blocks private/link-local IPs, rejects invalid URLs.

### Medium Priority

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
