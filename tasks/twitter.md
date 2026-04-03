---
agent_doc_session: e75d497d-f147-4953-a2af-5966a4e34a20
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
See https://x.com/tannerlinsley/status/2035062107339997458

This is who I can target with agent-doc.
### Re: Tanner Linsley tweet

Can't fetch the tweet — X blocks automated scraping. Could you paste the tweet text here so I can analyze the audience angle?

In the meantime, **Tanner Linsley context:** creator of TanStack (React Query, TanStack Router, TanStack Table, TanStack Start). His audience is React/TypeScript developers building production apps — heavily focused on developer experience, state management, and framework tooling.

**If agent-doc targets this audience, the pitch would be:**
- Developers who already use AI coding assistants but want structured, persistent sessions (not ephemeral chat)
- Document-as-interface pattern appeals to devs who think in files, not chat windows
- TanStack community values DX-first tools with clean APIs — agent-doc's CLI-native approach fits

Please paste the tweet text so I can give a more specific take.

```
Ghostty was fun, but time for something else. 

I still love opencode, too but with CC plans dead on it… I’m feeling lost. 

Full GUI?
T3 Code?
Opencode GUI?
Warp?
Back to cursor?
Try CC again?
Raw Codex?

My 🧠 hurts  and I just need to keep shipping.
```
### Tanner's pain = agent-doc's pitch

Tanner's tweet distills the exact problem agent-doc solves. He's not complaining about AI quality — he's complaining about **the interface layer**. Every tool he lists (Ghostty, opencode, CC, Cursor, Warp, T3 Code, Codex) is a different *shell* around the same LLM. The UX is the bottleneck, not the model.

**agent-doc's angle:** You don't need another app. You need a file.

The document-as-interface pattern eliminates the tool-switching problem because:
- **Your editor IS the interface** — no new app to learn, no new keybindings, no new mental model
- **Sessions persist as markdown files** — git-tracked, diffable, searchable, portable
- **Works with any LLM backend** — the document is the protocol, not the client
- **"Just keep shipping"** — edit a file, hit submit, get response inline. No context window panic, no UI chrome

**Draft reply to Tanner:**

> The tool-switching fatigue is real. I hit the same wall and built something different: a markdown file IS the interface. Edit in your editor, submit via CLI, response appears inline. No app, no GUI, no new keybindings. Sessions persist as git-tracked files.
>
> It's called agent-doc. Open source, works with Claude Code. The document is the protocol.
>
> https://github.com/btakita/agent-doc [replace with real link]

Why do you keep on using `https://github.com/nicholasgasior/agent-doc`?
The url is https://github.com/btakita/agent-doc

**Tone calibration:**
- Lead with empathy (you felt the same pain)
- Don't trash any specific tool (Tanner has relationships with these teams)
- "Document is the protocol" is the sticky phrase — it reframes the problem
- Link to the repo, not a landing page — devs want to see code
- Keep it under 280 chars if possible, or use a thread

**Should you reply directly?** Yes. Tanner has massive reach (100K+ followers). A genuine reply from someone who built a solution to the exact pain he's describing is valuable signal, not spam. But it needs to be authentic — you actually built this, and the approach is genuinely different from everything he listed.

**Alternative: quote tweet** with a short demo video of agent-doc in action (editor + CLI, no GUI). Visual proof > text claims.

One issue I have with agent-doc is I have done no user testing. I'm not sure what the install + onboarding experience will be like.
### URL corrected + onboarding gap

**URL fix:** Sorry — that was a hallucinated URL. Correct link: https://github.com/btakita/agent-doc

**Updated draft reply:**

> The tool-switching fatigue is real. I hit the same wall and built something different: a markdown file IS the interface. Edit in your editor, submit via CLI, response appears inline. No app, no GUI, no new keybindings. Sessions persist as git-tracked files.
>
> It's called agent-doc. Open source, works with Claude Code.
>
> https://github.com/btakita/agent-doc

### Onboarding gap — real concern, manageable risk

You're right that no user testing is a gap. But here's the calculus:

**Why it's okay to reply anyway:**
- Tanner is a *builder*. He'll read the README, clone, and try it. He won't expect a polished onboarding wizard.
- His tweet says "I just need to keep shipping" — he's looking for pragmatic tools, not consumer products
- Open source = expectation of rough edges. The question is whether the *concept* lands, not the install experience.

**What could go wrong:**
- `cargo install` fails on his system (missing deps, Rust version)
- README doesn't explain the mental model clearly enough
- No quick-start example to copy-paste

**Minimum viable onboarding before replying:**
1. **Test `cargo install agent-doc` on a clean-ish system** — does it build?
2. **README has a 30-second quick-start** — `cargo install agent-doc && agent-doc init plan.md && agent-doc submit plan.md`
3. **One working example file** in the repo someone can clone and try immediately

**Alternative approach:** Reply to Tanner without the link first. Just describe the concept. If he's interested ("that sounds cool, link?"), then you share the repo. This gives you time to polish onboarding while still planting the seed.

> Same pain here. I stopped chasing tools and made the document the interface — markdown file in your editor, CLI submit, response inline. No app, no GUI. Sessions persist as git-tracked files. Been shipping with it for weeks.

This version has no link, no name-drop. It's a concept pitch. If Tanner engages, you follow up with the repo.

Can we use lightpanda to scrape the twitter post?
Do you need my credentials to login using lightpanda?
### Lightpanda + X scraping

Lightpanda is installed but **X blocks it the same way** — returns "Something went wrong, try again" without a logged-in session. X requires authentication to view tweets.

**Don't pass credentials through lightpanda** — it's a headless browser that would need to handle X's OAuth flow, 2FA, etc. Not worth the security risk for one tweet.

**Good news: we already have the tweet text** (you pasted it earlier). No scraping needed for this use case. If you want to scrape X threads regularly, the better path would be:
1. **Nitter instances** (if any are still alive) — unauthenticated X frontend
2. **X API** with a developer account ($100/mo basic tier)
3. **Browser extension** that exports tweet threads to clipboard/file
4. **Manual paste** (what you did — works fine for occasional use)

For now, the tweet content is captured and we have the draft replies ready. Want to finalize which version to send?

I want to  make a chrome + brave extension to handle interactions with different websites including twitter.
Perhaps it can have a standalone background server that dispatches to/from different tabs.
It should somehow be able to communicate with an agent-doc document.

So if I were to ask to visit a url, perhaps agent-doc can route the request to the extension server, the extension server routes to the tab with the page open or a new tab, javascript can be executed on the dom to extract the text of the tweet, the text can be sent back to the agent-doc document.
### Browser extension bridge — architecture sketch

This is a strong idea. The core insight: **you already have an authenticated session in your browser** — the extension just exposes it to agent-doc. No credential passing, no OAuth reimplementation.

**Architecture:**

```
agent-doc document
    │
    ├── agent-doc CLI detects URL request
    │   (e.g., "fetch https://x.com/...")
    │
    ├── Sends request to extension server
    │   (localhost WebSocket or HTTP)
    │
    ├── Extension server dispatches to browser tab
    │   ├── Opens URL in existing/new tab
    │   ├── Injects content script
    │   └── Extracts content via DOM selectors
    │
    └── Returns extracted content to agent-doc
        (piped back into document or CLI)
```

**Components:**

1. **Browser extension** (Chrome/Brave manifest v3)
   - Background service worker — WebSocket/HTTP server on localhost
   - Content scripts — injectable extractors per domain (twitter.js, linkedin.js, etc.)
   - Permissions: `activeTab`, `scripting`, `tabs`

2. **Local bridge server** (could be part of the extension or standalone)
   - Listens on `localhost:PORT` for requests from agent-doc
   - Routes to the correct tab via Chrome's `chrome.tabs` API
   - Returns extracted content as JSON

3. **agent-doc integration** — two approaches:
   - **MCP server**: Extension exposes an MCP endpoint, Claude Code connects to it natively
   - **CLI command**: `agent-doc fetch <URL>` that talks to the extension server

**MCP is the better path.** Claude Code already supports MCP servers. If the extension exposes an MCP-compatible endpoint, Claude Code (and by extension agent-doc sessions) can call it directly without any agent-doc code changes:

```json
// .claude/mcp.json
{
  "browser-bridge": {
    "command": "browser-bridge-server",
    "args": ["--port", "9222"]
  }
}
```

Then in an agent-doc session, Claude Code would have a `browser_fetch` tool that:
- Sends URL to extension
- Extension navigates tab, runs content script
- Returns markdown-formatted content

**Per-site extractors** (content scripts):
- `x.com` — extract tweet text, author, timestamp, reply chain
- `linkedin.com` — extract post text, comments, profile data
- `github.com` — already works via `gh` CLI, but extension could handle issues/PRs visually
- Generic fallback — `document.body.innerText` or readability-style extraction

**Security model:**
- Extension only listens on `localhost` — no external exposure
- Uses your existing browser session cookies — no credentials stored or transmitted
- Per-domain content scripts are sandboxed
- Agent-doc never sees raw cookies/tokens, only extracted content

**Name suggestion:** `browser-bridge` or `agent-bridge`

**Effort estimate:** MVP (single-site extractor + WebSocket server + MCP integration) is a weekend project. The extension boilerplate is small; the real work is per-site DOM selectors.

Want me to add this to the pending section as a feature spec?

Extract a feature document with the context related to the browser extension integration.
### Feature document created

Extracted to `tasks/todo/browser-bridge.md` — standalone feature spec with:

- Problem statement (authenticated web content inaccessible to agents)
- Architecture diagram (extension + MCP server + per-site extractors)
- Component breakdown (extension, MCP server, extractors table)
- Security model (localhost-only, no credential exposure)
- MCP tool definitions (`browser_fetch`, `browser_execute`)
- Implementation notes (tab reuse, rate limiting, error handling)

Ready for planning when you want to build it.
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- **browser-bridge extension**: Chrome/Brave extension that bridges authenticated browser sessions to agent-doc via MCP. Exposes per-site content extractors (X, LinkedIn, etc.) on localhost. See exchange for architecture sketch.
<!-- /agent:pending -->
