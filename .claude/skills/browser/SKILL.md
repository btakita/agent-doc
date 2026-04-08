# browser

Drive the local Brave browser via Chrome DevTools Protocol (CDP).

## Tool Preference

**Use `chromium-bridge` CLI** for all browser automation (click, type, navigate, screenshot, markdown, snapshot). See the chromium-bridge skill for command reference.

Fall back to MCP tools (`mcp__chrome-devtools__*`) only for unsupported operations:

| Operation | Tool |
|-----------|------|
| Network request monitoring | `mcp__chrome-devtools__list_network_requests` |
| Console message capture | `mcp__chrome-devtools__list_console_messages` |
| Performance tracing | `mcp__chrome-devtools__performance_*` |
| Lighthouse audits | `mcp__chrome-devtools__lighthouse_audit` |
| File uploads | `mcp__chrome-devtools__upload_file` |
| Dialog handling | `mcp__chrome-devtools__handle_dialog` |
| Device emulation | `mcp__chrome-devtools__emulate` |

## Prerequisites

- Brave with `--remote-debugging-port=9222` (configured in `~/.config/brave-flags.conf`)
- `chromium-bridge` binary installed

Health check: `chromium-bridge check`

## Safety

- Port 9222 is localhost-only
- Never send credentials through evaluate
- LinkedIn: no bulk actions (mass messaging, connection scraping)
- Preview with screenshot before destructive actions

## Configuration

| Item | Location |
|---|---|
| Brave flags | `~/.config/brave-flags.conf` |
| MCP server | `~/.claude.json` → `mcpServers.chrome-devtools` |
| Debug port | `http://127.0.0.1:9222` |
