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
Hub-and-spoke architecture chosen | Git-backed server + Ragie sidecar | WebSocket protocol design next
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
<!-- agent:boundary:de0da3c1 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Design web prototype architecture (axum + CodeMirror + WebSocket + hub-and-spoke)
- [ ] Build web prototype MVP (markdown editor + Ragie retrieval + Claude responses)
- [ ] Define WebSocket message protocol (edit ops, document sync, agent responses)
- [ ] Test VS Code agent-doc extension
- [ ] Index demo documents in Ragie (patent proposal, contacts, resume)
- [ ] Prepare demo script for Bob call
- [ ] Explore Ragie Parse for Judith's loan docs and Ron's patent files
<!-- /agent:pending -->
