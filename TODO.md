# TODO

## Security Hardening

Items identified during a security audit (2026-03-23). Prioritized by impact.

### Medium Priority

- **ReDoS in evaluator regex**: Regex patterns from task JSON (`MatchMode::Regex`) are compiled without complexity limits. Consider adding a timeout or using `regex` crate's built-in linear-time guarantees (already the case — verify this is sufficient).

### Low Priority / Future Consideration

- **Process-level sandbox for execute-action.py**: The current `exec()` + restricted builtins approach has documented bypass vectors (attribute traversal, `__globals__`, module-level access). RestrictedPython or a seccomp-based sandbox would be stronger, though the container boundary remains the primary isolation.

- **VNC authentication option**: Add an optional `vnc_password` config field for users who need LAN-accessible VNC. Default is now localhost-only, but password auth would be useful for remote debugging workflows.

- **Image digest pinning**: Allow task JSON or config to specify image digests (`image@sha256:...`) for custom Docker images, enabling reproducible and tamper-resistant builds in CI.

- **TLS certificate pinning**: The HTTP client uses default TLS settings. For known API endpoints (api.openai.com, api.anthropic.com), certificate pinning would protect against MITM.
