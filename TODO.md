# TODO

## Security Hardening

Items identified during a security audit (2026-03-23). Prioritized by impact.

### High Priority

- ~~**`needs_fuse` config escape hatch**~~: Done — `needs_fuse: true` field added to `DockerImage` app config. When set, grants `CAP_SYS_ADMIN` and `/dev/fuse` to the container.

- ~~**Container network egress restrictions**~~: Done — `--no-network` global CLI flag sets Docker network mode to `"none"`, disabling all container networking.

- ~~**Sanitize API error responses**~~: Done — `sanitize_error_body()` helper truncates error bodies to 500 chars in both `http_base.rs` and `anthropic.rs`.

- ~~**SSRF via custom base URL**~~: Done — `api_base_url` is now validated in `Config::validate()`: requires HTTPS (except localhost), blocks private/link-local IPs, rejects invalid URLs.

### Medium Priority

- ~~**Prompt injection awareness**~~: Deprioritized — not addressing unless users demand it. The container boundary is the primary isolation; structured delimiters in prompts add complexity for marginal benefit.

- ~~**Evaluator temp file races**~~: Not a real bug — suite runs are strictly sequential (simple `for` loop with `.await`), so fixed filenames in `file_compare.rs` cannot collide. Will revisit if suite parallelization is added.

- **ReDoS in evaluator regex**: Regex patterns from task JSON (`MatchMode::Regex`) are compiled without complexity limits. Consider adding a timeout or using `regex` crate's built-in linear-time guarantees (already the case — verify this is sufficient).

- ~~**API key hygiene**~~: Done — stderr warning emitted when `api_key` is found in a config file, recommending environment variables instead.

### Low Priority / Future Consideration

- **Process-level sandbox for execute-action.py**: The current `exec()` + restricted builtins approach has documented bypass vectors (attribute traversal, `__globals__`, module-level access). RestrictedPython or a seccomp-based sandbox would be stronger, though the container boundary remains the primary isolation.

- **VNC authentication option**: Add an optional `vnc_password` config field for users who need LAN-accessible VNC. Default is now localhost-only, but password auth would be useful for remote debugging workflows.

- ~~**Monitor bind address override**~~: Done — `--monitor-bind-addr` CLI flag added (defaults to `127.0.0.1`, use `0.0.0.0` for remote access). See `docs/remote-monitoring.md`.

- **Image digest pinning**: Allow task JSON or config to specify image digests (`image@sha256:...`) for custom Docker images, enabling reproducible and tamper-resistant builds in CI.

- **TLS certificate pinning**: The HTTP client uses default TLS settings. For known API endpoints (api.openai.com, api.anthropic.com), certificate pinning would protect against MITM.
