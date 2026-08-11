# Release Notes

## Unreleased

- No changes yet.

## 0.1.37 - 2026-08-12

### Paseo Compatibility

- Added capability-based adapters for Paseo 0.2.x and 0.3.1 structured output, preserving complete permission and agent identifiers and failing closed for unsafe Control requests.
- Made activity parsing tolerant of new event kinds, redacted Thought content, and isolated malformed activity to the affected agent in monitor snapshots.

### Logging

- Added UTC RFC3339 date/time prefixes to MCP access, RPC, stdout, and stderr profile logs.

## 0.1.36 - 2026-08-09

### Desktop Stability

- Changed the tray reload action to recreate the WebView2 window from Rust, with concurrent recreation protection, instead of evaluating a reload script in an already-frozen renderer.

### MCP Catalog

- Added a real `text/event-stream` channel for `notifications/tools/list_changed`, retained JSON discovery for ordinary GET clients, and disabled caching on MCP responses.
- Paseo configuration saves still restart only the active workspace services; notification-aware clients now refresh their catalog automatically, while other clients receive an explicit reconnect hint.

### Paseo

- Added specific daemon, CLI timeout, partial parse, unknown activity, missing permission and missing agent classifications with bounded diagnostics and actionable recovery steps.
- Preserved recognized events when activity output is partially parseable, added 0.2.5 text/JSON regression fixtures, and verified the same guarded text format against Paseo 0.3.0.

## 0.1.35 - 2026-08-07

### Release Integrity

- Rebuilt the Windows release from the current `main` tree with the complete Paseo frontend and backend integration.
- Sidebar version display now reads the packaged Tauri runtime version, with the package version only as a development fallback.
- Changed repository, download, and update-check links to `LostInsight/coding-tools-mcp` so the desktop client no longer redirects users to an upstream build without Paseo.
- Added release verification that rejects mismatched package/Cargo/Tauri versions, stale frontend assets, missing Paseo Control tools, wrong update sources, or missing versioned MSI/NSIS bundles.

### Fixes

- Increased Cloudflare Named Tunnel readiness allowance to cover protocol fallback and terminate unready child processes on final failure.
- Replaced the Windows timeout test that force-terminated `ping.exe` with an isolated test helper, preventing `PING.EXE` `0xc0000142` popups during Rust test runs.

### Paseo

- Includes Paseo 0.2.5 activity parsing, workspace-scoped monitoring, Assist/Control modes, exact permission allow/deny, confirmed agent creation, and the desktop Paseo Integration configuration UI.


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
