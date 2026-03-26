---
agent_doc_session: a94c7d23-e171-482f-93f4-530b7f2c436f
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
LinkedIn automation via DevTools MCP documented. Ready to set up.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
How do I use the MCP Server for devtools?
https://developer.chrome.com/blog/new-in-devtools-146/#mcp-server
### Agent

Session initialized for `tasks/brave.md`. What would you like to work on?

### Re: Chrome DevTools MCP Server

The **Chrome DevTools MCP Server** (`chrome-devtools-mcp`) lets AI agents control a browser via MCP. It exposes 29+ tools for input, navigation, performance, debugging, network, and emulation.

**Setup for Claude Code:**
```bash
claude mcp add chrome-devtools --scope user npx chrome-devtools-mcp@latest
```

Or JSON config:
```json
{
  "mcpServers": {
    "chrome-devtools": {
      "command": "npx",
      "args": ["-y", "chrome-devtools-mcp@latest"]
    }
  }
}
```

**For Brave specifically** — use `--executablePath`:
```json
{
  "mcpServers": {
    "chrome-devtools": {
      "command": "npx",
      "args": [
        "chrome-devtools-mcp@latest",
        "--executablePath=/usr/bin/brave"
      ]
    }
  }
}
```

**Tool categories (29+):**
| Category | Tools |
|---|---|
| Input (9) | click, drag, fill, fill_form, handle_dialog, hover, press_key, type_text, upload_file |
| Navigation (6) | navigate_page, new_page, select_page, close_page, list_pages, wait_for |
| Performance (4) | start/stop traces, analyze insights, memory snapshots |
| Debugging (6) | take_screenshot, take_snapshot, evaluate_script, console messages, lighthouse_audit |
| Network (2) | list/get network requests |
| Emulation (2) | emulate device, resize page |

**Key flags:**
- `--browserUrl http://127.0.0.1:9222` — connect to running browser
- `--headless` — no UI
- `--isolated` — temp user-data dir (auto-cleaned)
- `--slim` — 3 core tools only (saves tokens)
- `--channel canary` — use Chrome Canary
- `--experimentalScreencast` — screen recording (requires ffmpeg)

Browser launches on first tool invocation, not on connection. Brave is at `/usr/bin/brave` on this system.

Repo: https://github.com/ChromeDevTools/chrome-devtools-mcp

Can I drive interactions with linkedin.com using the chrome mcp server?

### Re: Driving LinkedIn via Chrome MCP (HEAD)

Yes. The DevTools MCP server can drive any page you can open in the browser, including LinkedIn.

**What works:**
- `navigate_page` → open any LinkedIn URL
- `click` → expand comments, click buttons, open menus
- `fill` / `type_text` → search, compose messages
- `evaluate_script` → run arbitrary JS (same approach as the bookmarklet scraper)
- `take_screenshot` / `take_snapshot` → see page state or get full DOM
- `wait_for` → wait for elements to load before interacting
- `list_network_requests` → observe LinkedIn API calls

**For LinkedIn post scraping:** Navigate to post, `evaluate_script` with the bookmarklet JS, get markdown back. No manual clicking needed.

**Auth setup — connect to your running Brave:**

LinkedIn requires login. Best approach: attach to your already-authenticated Brave session.

1. Start Brave with remote debugging:
   ```bash
   brave --remote-debugging-port=9222
   ```
2. Configure MCP to attach:
   ```json
   {
     "mcpServers": {
       "chrome-devtools": {
         "command": "npx",
         "args": [
           "chrome-devtools-mcp@latest",
           "--browserUrl=http://127.0.0.1:9222"
         ]
       }
     }
   }
   ```

This gives the agent your existing LinkedIn session (cookies, auth, everything).

**Caution:** LinkedIn is mild about reading/browsing automation but aggressive about bulk actions (mass messaging, connection scraping). Keep interactions human-paced.
<!-- agent:boundary:e16970e6 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- Install chrome-devtools-mcp for Claude Code — if approved
- Set up Brave remote debugging launch shortcut — if wanted
<!-- /agent:pending -->
