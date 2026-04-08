---
agent_doc_session: 7350107c-817d-430e-b7bd-78a64a8fdc35
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---


# browser-bridge — Browser Extension for Agent-Doc

## Summary

Chrome/Brave extension that bridges authenticated browser sessions to agent-doc via MCP. Leverages existing browser cookies for authenticated access to sites like X, LinkedIn, etc. — no credential passing or OAuth reimplementation.

## Problem

AI coding tools (Claude Code, agent-doc) cannot access authenticated web content. Scraping tools (WebFetch, lightpanda) fail on sites requiring login (X, LinkedIn). Users must manually copy-paste content, breaking the agent workflow.

## Solution

A browser extension + local MCP server that:
1. Receives URL requests from Claude Code / agent-doc
2. Navigates to the URL in the user's authenticated browser
3. Extracts structured content via per-site DOM selectors
4. Returns markdown-formatted content to the agent

## Architecture

```
Claude Code / agent-doc
    │
    ├── MCP tool call: browser_fetch(url)
    │
    ├── MCP server (localhost process)
    │   └── WebSocket/HTTP to extension
    │
    ├── Browser extension (manifest v3)
    │   ├── Background service worker
    │   ├── Opens URL in existing/new tab
    │   ├── Injects content script
    │   └── Extracts content via DOM selectors
    │
    └── Returns extracted content as markdown
```

## Components

### 1. Browser Extension (Chrome/Brave manifest v3)

- **Background service worker** — WebSocket server on localhost, receives requests from MCP server
- **Content scripts** — per-domain extractors injected into tabs
- **Permissions:** `activeTab`, `scripting`, `tabs`

### 2. MCP Server (local process)

- Exposes MCP-compatible endpoint for Claude Code
- Forwards requests to extension via WebSocket
- Returns structured markdown responses

```json
// .claude/mcp.json
{
  "browser-bridge": {
    "command": "browser-bridge-server",
    "args": ["--port", "9222"]
  }
}
```

### 3. Per-Site Content Extractors

| Domain | Extracts |
|--------|----------|
| `x.com` | Tweet text, author, timestamp, reply chain |
| `linkedin.com` | Post text, comments, profile data |
| `github.com` | Issues, PRs (supplement to `gh` CLI) |
| Generic fallback | `document.body.innerText` or readability extraction |

## Security Model

- Extension only listens on `localhost` — no external exposure
- Uses existing browser session cookies — no credentials stored or transmitted
- Per-domain content scripts are sandboxed
- Agent-doc never sees raw cookies/tokens, only extracted content

## MCP Tools Exposed

### `browser_fetch`

Fetches and extracts content from a URL using the user's authenticated browser session.

**Parameters:**
- `url` (string, required) — URL to fetch
- `selector` (string, optional) — CSS selector to extract specific content
- `extractor` (string, optional) — named extractor (e.g., "tweet", "linkedin_post")

**Returns:** Markdown-formatted content

### `browser_execute`

Executes JavaScript in a browser tab and returns the result.

**Parameters:**
- `url` (string, required) — URL of the tab
- `script` (string, required) — JavaScript to execute

**Returns:** JSON result

## Implementation Notes

- **Tab reuse:** If a tab with the target URL is already open, reuse it instead of opening a new one
- **Rate limiting:** Respect per-domain rate limits to avoid triggering anti-automation
- **Error handling:** Return clear errors for blocked content, CAPTCHA challenges, etc.
- **Extension store:** Start with unpacked extension for development; consider Chrome Web Store later

## Origin

Sparked by inability to fetch X/Twitter content during an agent-doc session (tasks/twitter.md). Manual paste works but breaks the agent workflow loop.

## Status

Not started. MVP scope: X extractor + MCP server + Chrome extension.

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
