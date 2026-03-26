---
agent_doc_session: a94c7d23-e171-482f-93f4-530b7f2c436f
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
Browser skill architecture discussion. Recommending SKILL.md-first approach, CLI later if needed.
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

### Re: Driving LinkedIn via Chrome MCP

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

How can I have brave start the --remote-debugging-port by default?

### Re: Brave remote debugging by default

Brave reads `~/.config/brave-flags.conf` for extra command-line flags. Just add the flag there:

```bash
echo "--remote-debugging-port=9222" >> ~/.config/brave-flags.conf
```

That's it. Every Brave launch (from desktop, terminal, anywhere) will include `--remote-debugging-port=9222`. The file is one flag per line.

**Verify it works:** After restarting Brave, check:
```bash
curl -s http://127.0.0.1:9222/json/version | python3 -m json.tool
```

If you see a JSON response with `Browser`, `webSocketDebuggerUrl`, etc., it's working.

**Security note:** Port 9222 is only bound to localhost (`127.0.0.1`) by default, so only local processes can connect. No external exposure. But any local process (including malware) could control your browser through this port. If concerned, you could use a non-standard port (e.g., `--remote-debugging-port=9333`).

Want me to set this up now?
I restarted Brave. Please test and make necessary configuration changes.
### Re: Test remote debugging & configure

**Test result:** Port 9222 is **not responding**. The `brave-flags.conf` file didn't exist — Brave was running without the flag.

**Fixed:** Created `~/.config/brave-flags.conf`:
```
--remote-debugging-port=9222
```

**Note:** Brave binary is at `/opt/brave-bin/brave` (not `/usr/bin/brave` as I said earlier).

**Next step:** Restart Brave again for the flag to take effect. Then I'll verify the port and install the chrome-devtools-mcp server.

Restarted brave
### Re: Restarted brave

Remote debugging is **live**:
```json
{
    "Browser": "Chrome/146.0.7680.164",
    "Protocol-Version": "1.3",
    "webSocketDebuggerUrl": "ws://127.0.0.1:9222/devtools/browser/ffc7c99b-..."
}
```

**chrome-devtools-mcp installed** for Claude Code (user scope):
```
npx -y chrome-devtools-mcp@latest --browserUrl=http://127.0.0.1:9222
```

This connects to your running Brave session automatically. The MCP server will be available in all Claude Code sessions. You can now use browser tools like `navigate_page`, `evaluate_script`, `take_screenshot`, etc.

All setup is complete.

What SKILL.md do you recommend we should use to drive brave? agent-doc?
just a personal agent-loop skill? If that is the case, how can libraries such as agent-doc identify there is a skill for driving the browser?
### Re: Skill architecture for browser driving

**Not agent-doc** — agent-doc is a document session tool. Wrong scope.

**A personal `browser` skill** in `.claude/skills/browser/` is the right call. It would encapsulate high-level workflows on top of the raw MCP tools:

- "Scrape this LinkedIn post as markdown"
- "Take a screenshot of this URL"
- "Navigate and extract structured data from page X"

**How discovery works — it already does:**

Claude Code loads *all* MCP tools and *all* skills into every session. When you run `/agent-doc`, Claude (the agent) already has access to the chrome-devtools MCP tools (`navigate_page`, `evaluate_script`, etc.). agent-doc doesn't need to "discover" the browser — Claude orchestrates both.

The layer cake:
```
Skills (workflow)     → /browser scrape-linkedin-post <url>
                      → /agent-doc tasks/brave.md
MCP tools (capability) → chrome-devtools: navigate_page, evaluate_script, ...
Claude Code (agent)    → orchestrates everything, sees all tools + skills
```

So if agent-doc needs to drive the browser during a session, it doesn't call a browser skill — **Claude already has the MCP tools available** and uses them directly. A `/browser` skill is useful for *the user* to invoke repeatable browser workflows, not for inter-skill communication.

**What the browser skill would contain:**
- `SKILL.md` with workflow recipes (scrape, screenshot, form fill, etc.)
- Patterns for connecting to the authenticated Brave session
- Safety guardrails (rate limiting for LinkedIn, etc.)
- Common `evaluate_script` snippets (like the bookmarklet JS)

Want me to scaffold `.claude/skills/browser/SKILL.md`?

Should we have a rust library to distribute this skill? Are there deterministic cli functions that can be called, such as setup and other useful deterministic operations?
### Re: Rust CLI for browser skill?

**Probably not yet.** Here's the reasoning:

**agent-doc needed a CLI because** it manages complex state — snapshots, diffs, CRDT merges, git staging, IPC. Those operations are deterministic, performance-sensitive, and error-prone in shell.

**Browser operations are simpler.** The CDP HTTP API at `127.0.0.1:9222` is already CLI-accessible:

```bash
# List open tabs
curl -s http://127.0.0.1:9222/json/list

# Check health
curl -s http://127.0.0.1:9222/json/version

# Get debug WebSocket URL
curl -s http://127.0.0.1:9222/json/version | jq -r .webSocketDebuggerUrl
```

**Deterministic operations a CLI *could* handle:**
| Command | What it does |
|---|---|
| `browser setup` | Create brave-flags.conf, install MCP, verify port |
| `browser check` | Health check — is debugging port responding? |
| `browser list` | List open tabs with URLs |
| `browser screenshot <url>` | Headless screenshot (no LLM needed) |

But these are all thin wrappers around `curl` + file writes. The ROI on a Rust crate is low right now.

**Recommendation:** Start with **SKILL.md only**. The skill handles orchestration (LLM-driven workflows), and shell commands handle the deterministic parts inline. If you find yourself repeating the same 5+ line shell sequences across sessions, *then* extract a CLI.

The pattern: **SKILL.md first → shell helpers → CLI crate** (only if complexity warrants it).

Want me to scaffold the SKILL.md now?
<!-- agent:boundary:42de2503 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- Scaffold `.claude/skills/browser/SKILL.md` — awaiting approval
<!-- /agent:pending -->
