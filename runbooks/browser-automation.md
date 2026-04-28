# Browser Automation: chromium-bridge CLI vs Chrome DevTools MCP

## Default: chromium-bridge CLI

Use the `chromium-bridge` CLI binary (in `.bin/`) for all browser automation tasks. It connects directly to the browser via CDP and is more reliable than the Chrome DevTools MCP server.

### Common operations

```bash
chromium-bridge check                    # health check
chromium-bridge list                     # list open tabs
chromium-bridge navigate URL             # go to a URL
chromium-bridge screenshot [--output F]  # capture screenshot
chromium-bridge markdown                 # page content as markdown
chromium-bridge snapshot                 # accessibility tree dump
chromium-bridge click SELECTOR           # click an element
chromium-bridge type TEXT                # type into focused element
chromium-bridge evaluate 'js expression' # run JavaScript
chromium-bridge select-tab PATTERN       # switch tabs
chromium-bridge wait SELECTOR            # wait for element
```

Add `--json` for machine-readable output.

### When to use

- Navigating to dashboards (Cloudflare, GA4, GTM, etc.)
- Taking screenshots for analysis
- Filling forms, clicking buttons
- Extracting page content as markdown
- Any task that needs browser interaction

## Fallback: Chrome DevTools MCP

The `mcp__chrome-devtools__*` tools connect through an MCP server layer. Use only when:

- `chromium-bridge` is unavailable (binary missing or browser not running)
- A specific MCP capability has no CLI equivalent (e.g., `lighthouse_audit`, `performance_start_trace`, `emulate`)

### Known issues

- `Network.enable timed out` errors are common when the MCP server connection is stale
- Requires the MCP server process to be running and healthy
- Adding an extra network hop compared to direct CDP

## Setup

The browser must have remote debugging enabled on port 9222 (default). Run `chromium-bridge setup` for configuration instructions. Verify with `chromium-bridge check`.
