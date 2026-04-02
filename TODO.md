# TODO

## Security Hardening

Items identified during a security audit (2026-03-23). Prioritized by impact.

### Low Priority / Future Consideration

- **VNC authentication option**: Add an optional `vnc_password` config field for users who need LAN-accessible VNC. Default is now localhost-only, but password auth would be useful for remote debugging workflows.

- **TLS certificate pinning**: The HTTP client uses default TLS settings. For known API endpoints (api.openai.com, api.anthropic.com), certificate pinning would protect against MITM.
