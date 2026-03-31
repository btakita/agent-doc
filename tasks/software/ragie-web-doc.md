---
agent_doc_session: 4ef1150a-7b51-4ff6-9284-9c7c1f1570e5
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
links:
  - ../professional/bob-remeika.md
---

## Status

<!-- agent:status patch=replace -->
"Always local, optionally web" architecture | `agent-doc serve` subcommand designed | Implementation planning next
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Re: New session — ragie-web-doc

Fresh document with empty components. What's the goal here? Add your prompt to the exchange component and I'll pick it up on the next cycle.

Link this document with tasks/professional/bob-remeika.md
### Re: Linking with bob-remeika.md

Cross-reference established. Here's the relevant context from `tasks/professional/bob-remeika.md`:

**Web prototype plan (from bob-remeika exchange):**
- MVP: markdown editor pane + AI response pane + Ragie retrieval side panel
- Tech: basic HTML frontend + backend proxying Claude API + Ragie API
- Demo docs to index: patent mining proposal, contact profiles, resume
- Demo script: edit a draft → agent pulls context from Ragie → shows retrieval results

**Ragie Parse use cases:**
- **Judith** — commercial real estate loan docs (appraisals, compliance)
- **Ron** — patent corpus (complex PDF layouts, tables, multi-column)

**Demo assets:**
- JetBrains plugin (ready now)
- VS Code (needs testing)
- Web prototype (to build)

**Meeting plan:** 30-min call with Bob. JB plugin demo → Ragie pitch/web prototype → discussion → contract work → next steps.

Add your requirements or next steps to the exchange and I'll pick them up.
I thought agent-doc supports linked documents in the frontmatter.
### Re: Linked documents via related_docs frontmatter

You're right — agent-doc has a `related_docs` frontmatter field. Adding it to the frontmatter will cause preflight to surface changes in `bob-remeika.md` each cycle:

```yaml
related_docs:
  - ../professional/bob-remeika.md
```

This is better than a manual cross-reference — preflight will automatically detect when bob-remeika.md changes and include a summary in the preflight output. I'll add it to the frontmatter now.

Can we make agent-doc work in a browser view?
Can the editing work across the network?
Should we use WebRTC or plain web sockets?
### Re: Browser view and network editing

Great questions — these connect directly to the web prototype MVP from the Bob conversation.

**Can agent-doc work in a browser view?**

Yes, and the architecture maps cleanly:

1. **Backend server** — serves the markdown file, runs `agent-doc preflight`/`write`/`commit` on the server side
2. **Browser frontend** — CodeMirror 6 or Monaco editor rendering the markdown, with split pane for rendered preview
3. **Real-time sync** — WebSocket pushes file changes to the browser; browser sends edits back to the server
4. **Agent loop** — server-side process watches for changes (same diff→respond→write cycle), pushes responses via WebSocket

The CRDT merge (`--stream`) already handles concurrent edits, so the hardest part (conflict resolution) is solved. The browser just becomes another editor, like VS Code or JetBrains.

**Can editing work across the network?**

Yes — the server runs wherever the git repo lives. Browser clients connect over HTTP/WebSocket. This gives you:
- **Remote pair editing** — Bob sees the document live while you edit
- **Demo mode** — share a URL, Bob watches the agent respond in real-time
- **Mobile access** — edit from a phone/tablet

The minimal stack: a Rust HTTP server (axum/actix) wrapping the existing `agent-doc` CLI, serving a static frontend. No new state management needed — the file system + git remain the source of truth.

**"Should w"** — looks like your thought got cut off. What were you going to ask?

**Related docs link** — the `related_docs` frontmatter is now active. Preflight will surface bob-remeika.md changes automatically.
### Re: WebRTC vs plain WebSockets

**Plain WebSockets — use those.** Here's why:

**WebRTC is overkill for this use case.** WebRTC solves peer-to-peer media streaming with NAT traversal, STUN/TURN servers, and codec negotiation. Agent-doc is text-only, server-mediated, and doesn't need P2P. WebRTC adds complexity (signaling server, ICE candidates, DTLS) with zero benefit.

**WebSockets are the right fit because:**
- **Server-mediated architecture** — all edits flow through the server (where git, agent-doc CLI, and Claude API live). No peer-to-peer needed.
- **Simple protocol** — bidirectional text frames over a single TCP connection. Native browser API, trivial Rust support (tokio-tungstenite or axum's built-in WS).
- **Low latency** — persistent connection, no polling. Sub-50ms round trip for edit operations.
- **Battle-tested for collaborative editing** — CodeMirror's collab extension, Yjs, and Automerge all use WebSockets as the default transport.

**Proposed message flow:**
```
Browser ──ws──> Server ──file write──> agent-doc write --stream
                Server <──inotify/poll── file change detected
Browser <──ws── Server (push updated document)
```

The server watches the file (inotify on Linux) and pushes changes. The browser sends edit operations as WebSocket messages. The server applies them via `agent-doc write --stream` (CRDT merge handles conflicts).

**Good call on the `links` rename** — that matches agent-doc v0.28.3's field rename from `related_docs` to `links`.

How would this look for web collaboration? Could Ragie, or a file server, host the source document?
How would websockets come into play? Should we consider P2P or have a service broker all communication?
### Re: Web collaboration architecture — document hosting and communication topology

**Could Ragie or a file server host the source document?**

Two viable architectures, depending on what you want:

#### Option A: Git-backed server (recommended for MVP)

The server owns the git repo and the markdown files. Ragie is a **retrieval sidecar**, not the document store.

```
Browser ──ws──> agent-doc-web (axum)
                  ├── filesystem: markdown files + git
                  ├── agent-doc CLI: preflight/write/commit
                  ├── Claude API: agent responses
                  └── Ragie API: context retrieval (read-only)
```

- Documents live on the server's filesystem, versioned by git
- Ragie indexes documents for retrieval but doesn't own them
- This is the simplest path — reuses everything agent-doc already does
- Server can be a VPS, a Cloudflare Worker + Durable Object, or even localhost with a tunnel (cloudflared/ngrok)

#### Option B: Ragie as document store (future)

Ragie becomes the source of truth. Documents are stored in Ragie, fetched/pushed via their API. Git becomes optional (or a sync target).

- Pros: no server filesystem to manage, Ragie handles indexing automatically
- Cons: Ragie's API isn't designed for real-time collaborative editing, no CRDT/OT support, adds latency
- Makes sense later if Ragie adds a document editing API, but not for MVP

**Recommendation:** Option A for MVP. Git + filesystem is the proven foundation. Ragie augments with retrieval, not storage.

**P2P vs service broker?**

**Service broker (centralized server), definitely.** Reasons:

1. **Git requires a single writer** — concurrent git operations from P2P clients would corrupt the repo. A central server serializes writes.
2. **Claude API calls need a server** — API keys shouldn't live in the browser. The server proxies Claude requests.
3. **Ragie API is server-side** — same reason. No API keys in the browser.
4. **Simpler auth** — server controls who can read/write. P2P needs distributed auth (hard problem).
5. **WebSocket fan-out is trivial** — server receives an edit, applies it, broadcasts to all connected clients. This is ~50 lines of axum code.

The architecture is a **hub-and-spoke** model:

```
Client A ──ws──┐
Client B ──ws──├──> agent-doc-web server ──> git + Claude + Ragie
Client C ──ws──┘
```

Each client gets a WebSocket connection. Edits from any client are applied server-side and broadcast to all others. The agent-doc CRDT merge handles conflicts if two clients edit simultaneously.

**P2P only makes sense if** you need to work without a server (offline-first, mesh networking). That's not this use case.

I would like agent-doc local and agent-doc web to seamlessly integrate.
Would it be fair to make agent-doc web enabled in addition to local fs support (current status)?
### Re: Seamless local + web integration

Yes — and this is the elegant design. agent-doc shouldn't be "local mode OR web mode." It should be **always local, optionally web-accessible.**

**The key insight: the web server is a thin layer on top of the existing local architecture.** Nothing changes about how agent-doc works — the filesystem, git, snapshots, CRDT merge, preflight/write/commit all stay exactly as they are. The web server just exposes the same operations over HTTP/WebSocket.

#### How it works concretely

```
                    ┌─── JetBrains plugin (inotify + IPC)
                    ├─── VS Code extension (inotify + IPC)
 agent-doc CLI ─────├─── tmux/terminal (direct CLI)
 (filesystem + git) ├─── agent-doc-web (axum, inotify + WebSocket)
                    │      └─── Browser A, Browser B, ...
                    └─── Claude Code skill (/agent-doc)
```

All clients — IDE plugins, terminal, browser — are peers. They all read/write the same files on disk. The web server is just another client that proxies filesystem operations to/from browsers.

#### Implementation: `agent-doc serve`

A new subcommand that starts the web server:

```bash
agent-doc serve [--port 8080] [--host 0.0.0.0] [path/to/document.md]
```

What it does:
1. **Serves the document** as a web page (CodeMirror editor + rendered preview)
2. **Watches the file** via inotify — when any local client edits (IDE, CLI), pushes the change to all connected browsers
3. **Accepts browser edits** via WebSocket — writes them to disk via `agent-doc write --stream`, which triggers inotify for IDE plugins
4. **Proxies agent-doc operations** — preflight, write, commit are all local CLI calls under the hood

The beauty: **you can edit in JetBrains while Bob watches in the browser.** Both see the same live document. Agent responses appear in both simultaneously because they're all watching the same file.

Can we make this accessible via cli or mpc for Bob to also interact with using his own Claude code environment. Without having to install agent-doc?
Please present some options.

#### What stays the same

- `agent-doc preflight` — unchanged, reads filesystem
- `agent-doc write --stream` — unchanged, CRDT merge to filesystem
- `agent-doc commit` — unchanged, git operations
- Snapshot storage — unchanged, `.agent-doc/snapshots/`
- IPC with IDE plugins — unchanged, same mechanism

#### What's new

- `agent-doc serve` — axum HTTP server + WebSocket endpoint
- Static frontend assets — CodeMirror 6 editor, bundled into the binary
- WebSocket protocol — edit operations, document sync, cursor positions
- Optional: auth (API key or session token for remote access)

**So yes — "web-enabled" is the right framing.** Local is the foundation. Web is an optional access layer. No mode switching, no configuration changes. Just `agent-doc serve` and share the URL.
### Re: Remote access for Bob via CLI or MCP — without installing agent-doc

Three options, from simplest to most capable:

---

#### Option 1: MCP Server (recommended)

Expose agent-doc operations as an MCP server that Bob's Claude Code connects to remotely.

```
Bob's Claude Code ──MCP (stdio over SSH or HTTP)──> your machine
                                                      └── agent-doc CLI
```

**How it works:**
- You run `agent-doc mcp-server` (new subcommand) which exposes tools: `read_document`, `write_document`, `preflight`, `list_documents`
- Bob adds your MCP server to his Claude Code config
- Bob's Claude Code can now call these tools natively... no agent-doc binary needed on his end
- Transport options:
  - **SSH stdio** — Bob's Claude Code spawns `ssh brian@host agent-doc mcp-server` (zero infra, works today with any SSH access): Do I need a ssh tunnel with authorized users? Is it possible to make this network seamless + secure? Perhaps by using a password auth instead. Ideally a cli command to run the server and generate a link.
  - **HTTP/SSE** — `agent-doc serve --mcp` exposes an HTTP-based MCP endpoint (needs the web server): This could also be a https endpoint when hosted on a server.: Does this use a websocket?
  
**Pros:** Native Claude Code integration, Bob gets tool use with his own model/context, no new binary to install, MCP is the standard protocol.: We could optionally provide access...but I'm concerned about prompt injection. It would be better to just have Bob use his own Claude code session for now. A more recure sandbox can be provided later.
**Cons:** Needs SSH access or an HTTP endpoint.

Bob's setup would be one config entry:
```json
{
  "mcpServers": {
    "agent-doc-brian": {
      "command": "ssh",
      "args": ["brian@host", "agent-doc", "mcp-server", "--project", "/path/to/agent-loop"]
    }
  }
}
```

---

#### Option 2: WebSocket CLI bridge (lightweight)

A thin CLI wrapper connects to `agent-doc serve` via WebSocket... no agent-doc install needed.

```
Bob's machine:  npx agent-doc-client ws://brian:8080/doc
Bob's Claude Code ──/agent-doc──> local wrapper ──ws──> your server
```

- Bob runs a tiny client (npm package, single Python script, or `websocat`) that mirrors the document to a local temp file
- Bob's Claude Code uses the standard `/agent-doc` skill on the local temp file
- Edits sync bidirectionally via WebSocket

**Pros:** Works with existing `/agent-doc` skill. Minimal client.
**Cons:** Two-hop latency. Needs the web server built first.

---

#### Option 3: Pure HTTP API (no client needed)

Bob interacts entirely through Claude Code's `WebFetch` or a custom MCP wrapper.

```
Bob's Claude Code ──WebFetch──> https://brian.example.com/api/doc/ragie-web-doc
```

- `agent-doc serve` exposes REST: `GET /api/doc/:name`, `POST /api/doc/:name`, `POST /api/doc/:name/preflight`
- Zero setup on Bob's end

**Pros:** Zero installation. Works from any environment.
**Cons:** No real-time push. Clunky UX compared to MCP.

---

#### Recommendation: Option 1 (MCP Server)

MCP is purpose-built for this. Bob adds one line to his config and gets native tool access. The SSH transport works immediately with zero infrastructure. Later, `agent-doc serve` adds HTTP transport for the same MCP server.

**Implementation order:**
1. `agent-doc mcp-server` — stdio-based MCP server (works over SSH today)
2. `agent-doc serve --mcp` — HTTP-based MCP endpoint (when the web server lands)
3. WebSocket document sync (for real-time browser editing)
### Re: MCP transport security, HTTPS, and prompt injection concerns

**SSH — no tunnel needed, but the UX is clunky.** Better approach:

**The ideal UX: `agent-doc serve --share`**

```bash
$ agent-doc serve --share tasks/software/ragie-web-doc.md
🔗 Share this MCP endpoint:
   https://ad-7f3k2x.agent-doc.dev

   Token: ad_live_7f3k2xQ9m...
   Expires: 24h

   Bob's config:
   {
     "mcpServers": {
       "brian-agent-doc": {
         "url": "https://ad-7f3k2x.agent-doc.dev/mcp",
         "headers": { "Authorization": "Bearer ad_live_7f3k2xQ9m..." }
       }
     }
   }
```

Uses a relay (cloudflared tunnel or self-hosted) to expose the local MCP server over HTTPS with a generated token. No SSH keys, no port forwarding. Bob pastes one config block.

**Practical options for MVP (in order of simplicity):**
1. **cloudflared tunnel** — `agent-doc serve --tunnel` wraps `cloudflared tunnel` for instant HTTPS URL. Free, no account needed.
2. **Token auth** — `agent-doc serve --token <secret>` on a VPS with TLS. Bob connects with the token.
3. **SSH** — fallback for same-network or existing SSH access.

**HTTP/SSE — yes HTTPS, no WebSocket for MCP.** The MCP "Streamable HTTP" transport uses HTTP + Server-Sent Events (SSE) for server→client streaming, regular HTTP POST for client→server. WebSocket is reserved for the browser collaboration layer (real-time document editing), which is a separate concern from the MCP tool protocol.

**Prompt injection — your instinct is correct.** Bob's Claude Code sending tool calls to your MCP server is a surface. The safe architecture:

- Bob gets **scoped MCP access** (read + write to specific documents only)
- Bob uses his **own Claude Code session** (his model, his API key, his context)
- Your agent-doc remains local — Bob's edits come through `write_document` but are applied via CRDT merge (text only, no code execution)
- Future sandbox: per-tool permissions, audit logging, rate limiting, content filtering

**For MVP:** Bob connects via MCP, gets `read_document` + `write_document` scoped to specific files, uses his own Claude Code. No shared model context. The only prompt injection vector is document content itself, which is mitigated by document-level scoping.
<!-- agent:boundary:c43e32af -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Design MCP server tool schema (`read_document`, `write_document`, `preflight`, `list_documents`)
- [ ] Implement `agent-doc mcp-server` subcommand (stdio transport)
- [ ] Add `--share` flag with cloudflared tunnel integration for instant HTTPS sharing
- [ ] Add token-based auth for MCP endpoints
- [ ] Add document-level scoping for MCP access control
- [ ] Test MCP over SSH with a second Claude Code instance
- [ ] Implement `agent-doc serve` subcommand (axum + inotify + WebSocket)
- [ ] Add `--mcp` flag to `agent-doc serve` for HTTP/SSE-based MCP transport
- [ ] Build CodeMirror 6 frontend (editor + markdown preview + WebSocket client)
- [ ] Define WebSocket message protocol (edit ops, document sync, cursor positions)
- [ ] Bundle frontend assets into agent-doc binary (rust-embed or include_bytes)
- [ ] Add Ragie retrieval panel to web frontend
- [ ] Test VS Code agent-doc extension
- [ ] Index demo documents in Ragie (patent proposal, contacts, resume)
- [ ] Prepare demo script for Bob call
- [ ] Explore Ragie Parse for Judith's loan docs and Ron's patent files
<!-- /agent:pending -->
