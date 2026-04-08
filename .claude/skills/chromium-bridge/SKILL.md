# chromium-bridge

CLI for browser automation via Chrome DevTools Protocol. Direct WebSocket — no Puppeteer overhead.

## Commands

| Command | Description |
|---------|-------------|
| `check` | Health check |
| `list` | List open tabs |
| `navigate <url>` | Navigate to URL |
| `evaluate '<js>'` | Run JavaScript in tab |
| `screenshot` | Capture screenshot |
| `markdown <url>` | Web page to markdown |
| `click '<selector>'` | Click element by CSS selector |
| `type '<selector>' '<text>'` | Type into element |
| `select-tab '<pattern>'` | Activate tab by index or URL/title pattern |
| `wait '<selector>'` | Wait for selector (default 10s) |
| `snapshot` | Accessibility tree dump |
| `setup` | Configure browser for debugging |

## Tab Selector

`--tab` accepts index (`--tab 0`) or pattern (`--tab messenger`, substring match on URL/title). Ambiguous patterns error with all matches listed.

## Type

Double-newlines (`\n\n`) become Shift+Enter — line breaks without triggering send in contenteditable fields.

## Rich Text (Chat Apps)

For formatted messages (bold, paragraphs), use `evaluate` with `innerHTML`:

```bash
chromium-bridge evaluate --tab messaging '
var input = document.querySelector("div[contenteditable=true]");
input.innerHTML = "<p><strong>Header:</strong> text.</p><p><br></p><p>Next paragraph.</p>";
input.dispatchEvent(new Event("input", {bubbles: true}));
"done";
'
```

**Selectors:** LinkedIn: `div.msg-form__contenteditable`, Gmail: `div[aria-label="Message Body"]`, Messenger: `div[role=textbox]`

## Configuration

| Item | Default |
|------|---------|
| CDP host | `127.0.0.1` (env: `CHROMIUM_BRIDGE_HOST`) |
| CDP port | `9222` (env: `CHROMIUM_BRIDGE_PORT`) |
| Timeout | `5000ms` (`--timeout`) |

Requires Chromium-based browser with `--remote-debugging-port=9222`. Run `chromium-bridge setup` to configure.
