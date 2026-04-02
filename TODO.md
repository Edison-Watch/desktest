# TODO

## Security Hardening

Items identified during a security audit (2026-03-23). Prioritized by impact.

### Low Priority / Future Consideration

- **Process-level sandbox for execute-action.py**: The current `exec()` + restricted builtins approach has documented bypass vectors (attribute traversal, `__globals__`, module-level access). RestrictedPython or a seccomp-based sandbox would be stronger, though the container boundary remains the primary isolation. **Phase 1 done:** Docker container hardening (cap_drop ALL, no-new-privileges, SYS_ADMIN only when FUSE is needed) is now in place as a baseline defense-in-depth layer.

- **VNC authentication option**: Add an optional `vnc_password` config field for users who need LAN-accessible VNC. Default is now localhost-only, but password auth would be useful for remote debugging workflows.

- ~~**Image digest pinning**~~: Implemented — `DockerImage` app config now supports an optional `digest` field (`sha256:<64 hex>`). The digest is validated at parse time, verified against `repo_digests` after pull, and hard-errors on mismatch. Inline digests in the `image` field (`image@sha256:...`) are also supported.

- **TLS certificate pinning**: The HTTP client uses default TLS settings. For known API endpoints (api.openai.com, api.anthropic.com), certificate pinning would protect against MITM.
