# Release Notes

## Unreleased

- No changes yet.

## 0.1.34 - 2026-08-06

### Desktop Stability

- Integrated upstream `v0.1.33` and retained its Windows WebView recreation fix for minimized-window sentinel coordinates.
- Added regression coverage for invalid off-screen positions and unusable window sizes during UI recreation.

### Paseo Integration

- Added an opt-in, workspace-scoped Paseo CLI integration with read-only, Assist, and Control access modes.
- Added eleven fixed `paseo_` MCP/Actions tools, including exact permission allow/deny and confirmed background agent creation.
- Added Paseo 0.2.5 activity compatibility for line-oriented `[Kind]` logs and valid empty timelines.
- Added bounded direct process execution, versioned parsing, redaction, rate limits, audit events, and application-data snapshot retention.
- Added an independent desktop configuration area and detailed setup/security documentation.
- Existing workspace profiles migrate with Paseo disabled, and existing core tool catalogs remain unchanged when disabled.
- Verified real Paseo 0.2.5 permission requests with exact request IDs; the MCP surface intentionally does not expose `--all`.
