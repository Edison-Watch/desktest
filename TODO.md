# TODO

## Security Hardening

Items identified during a security audit (2026-03-23). Prioritized by impact.

### Low Priority / Future Consideration

- **TLS certificate pinning**: The HTTP client uses default TLS settings. For known API endpoints (api.openai.com, api.anthropic.com), certificate pinning would protect against MITM.
