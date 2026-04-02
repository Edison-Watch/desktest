# TODO

## Security Hardening

Items identified during a security audit (2026-03-23). Prioritized by impact.

### Low Priority / Future Consideration

- **Process-level sandbox for execute-action.py**: The current `exec()` + restricted builtins approach has documented bypass vectors (attribute traversal, `__globals__`, module-level access). RestrictedPython or a seccomp-based sandbox would be stronger, though the container boundary remains the primary isolation.

- **Image digest pinning**: Allow task JSON or config to specify image digests (`image@sha256:...`) for custom Docker images, enabling reproducible and tamper-resistant builds in CI.
