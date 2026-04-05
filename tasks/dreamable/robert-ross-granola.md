---
agent_doc_session: b70be1e0-5f53-420d-862d-07f50661c114
agent_doc_format: template
agent_doc_write: crdt
---


## Meeting: Brian Takita / Rob Ross

**Date:** April 2, 2026
**Author:** Robert Ross (via Granola)
**Source:** [Granola shared note](https://notes.granola.ai/d/e4d8dd00-6468-4a0b-93f1-df1f7d84d956)

Can we make this bidirectional (granola <-> agent-doc) in any way?

## Notes

<!-- agent:notes patch=replace -->
### Brian Takita — Background & Context

- Recently laid off from Presence (AI avatar startup), which shut down abruptly the same day
  - Funded by Andre Taranovsky (Chat Roulette founder); ~5-6 person team at peak, 2-3 at close
  - Built real-time lip-syncing avatars using MuseTalk -> LatentSync; used LiveKit cloud for WebRTC
- Currently building personal tooling and taking small consulting clients (~$100-150/hr)
- Previously at Pivotal Labs (Ruby on Rails; clients included early Twitter, eBay)
- Started using Claude Code ~1 month ago; has been building and posting YouTube videos about it

### Agent Doc — Brian's Key Project

- Custom interactive document harness built in Rust (converted from Python via Claude)
- Markdown file acts as a bi-directional interface with Claude Code via tmux + IDE plugin (JetBrains/VS Code)
  - Uses CRDT (conflict-free replicated data types) for collaborative-style editing
  - Tracks git diffs and document snapshots per interaction
  - tmux router bridges the document to the active Claude Code session
- Vision: any AI-generated artifact becomes interactive and annotatable inline
- All interactions stored as git history -> enables session analysis (Brian used this for a blog post analyzing 24hrs of themes)
- Open source (MIT license); currently single-user/local; web version in early exploration
  - Prompt injection risk identified as blocker for multi-user collaboration

> - Vision: any AI-generated artifact becomes interactive and annotatable inline

Can we use agent-docs to economically provide the interface for any AI-generated artifact to interact with the agent harness?

We are starting with Granola in this example. But we can expand to other areas such as Figma Jam, Build Party, Shimmer.
Allow for free form agent harness interaction within a variety of web interfaces.

### Existence Lang — Brian's Philosophy Framework

- Ontology system developed in 2015; has a Rust crate and binary
- Defines canonical, scope-aware definitions of words from the perspective of "existence" as universal set
  - Prevents meaning drift when the same word is used across different contexts/systems
- Applied to Agent Doc: each project gets its own ontology (e.g., what does "route" mean in Agent Doc?)
- Rob connected this directly to the taxonomy/ontology work he discussed with Miguel for Build Party

### Fit & Alignment Notes

- Strong conceptual overlap: Agent Doc <-> Shimmer (session viewer/analyzer); ontology work <-> Build Party taxonomy
- Brian's approach to agentic development (control, understanding over automation) aligns with Rob's philosophy
- Brian comfortable with AWS, LiveKit (used cloud + self-hosted for dev/CI at Presence), WebRTC
- Compensation context: was making $230K/yr at Presence; consulting rate $100-150/hr; flexible given circumstances
- Rob's read: strongest candidate conversation to date; wants to move to a short paid engagement

### Products Covered (Rob's Pitch)

**Build Party:** live infrastructure for AI builder communities
- Watch party, pre/post-party, co-build rooms, competitions (vibe coding hackathons), courseware: Livekit-enable components that bring interactivity to a variety of events. Polls, agent harness interactions, deep bi-directional surveys, hackathons (collaborative? security concerns?), courses to learn Livekit and other technologies within a sandboxed agent harness
- Built on LiveKit; targeting tool providers (e.g., LiveKit) as first strategic partners: bidirectional a/v interactions with other humans and agents. interactive design + coding sessions. collaborative high-trust work environments...if we provide a sandbox, then it safely protects against malicious prompts.
- LiveKit raised $100M in Jan 2026; Neil Dwyer (built LiveKit agent framework) is an advisor

**Shimmer:** developer sidekick/helper app
- View, back up, and semantically search Claude Code sessions and assets
- Share sessions with collaborators (e.g., Rob seeing Miguel's daily output): how can we have daily blog posts for interesting topics from all work activity...including git commits, claude sessions, other connected services.
- Memory layer via Cor.e or Vespa; semantic search via Trieve (acquired by Mintlify): Describe Cor.e and how Mintlify integrates Trieve
- Later: session scoring, improvement suggestions, cross-session intelligence

### Tools & Services Discussed

- **Granola:** note-taking app Rob uses (and wants to build similar functionality into Build Party)
- **Bench:** Claude Code session sharing (won Product Hunt ~1 week ago; Rob using but finds it limited)
- **Claude.bin:** session sharing with MIT license; early stage
- **Cor.e:** persistent memory MCP; Rob and Miguel share one login
- **Vespa:** Apache-licensed alternative to Cor.e; better cross-session capability
- **Trieve:** semantic + full-text search (acquired by Mintlify)
- **Raggy:** RAG backend (Brian spoke with CEO Bob day-of)
- **Every.to (Dan Shipper):** AI builder community + products built with Claude Code; Rob listens regularly
- **Study Stream:** YC-backed co-working app; one of LiveKit's largest customers
- **Proof:** collaborative markdown editor (Rob shared link with Brian)
- **Zed editor:** Brian flagged as worth exploring for shared context / collaboration
<!-- /agent:notes -->

## Analysis

<!-- agent:analysis patch=replace -->
<!-- /agent:analysis -->

## Action Items

<!-- agent:action-items patch=replace -->
**Rob:**
- Define two scoped starter projects over the weekend (one Build Party-side, one Shimmer-side)
  - Build Party: evaluate 3-5 GitHub repos (LiveKit streaming starter + Discord alternatives built on LiveKit); read Build Party PRD; make a recommendation
  - Shimmer: skeleton app to parse Claude Code sessions, set up Trieve + Cor.e or Vespa, surface sessions and outputs to Rob in near-real-time
- Agree on hourly rate to kick off (~$100/hr discussed as starting point)
- Text Brian to confirm Sunday 5pm ET call

**Brian:**
- Prepare for Sunday follow-up (5pm ET / 2pm PT, Apr 5) — placeholder, confirm via text
  - Easter may affect timing; evening works if needed
- Create a mermaid diagram of the step-by-step process for building a course (for Miguel's LiveKit agent course)
- Consider doing a deeper Claude Code setup walkthrough/demo for Rob (screen share of actual workflow)

**Miguel (noted for follow-up):**
- Post Claude Code session to Bench in Slack at end of each day so Rob can review
- Complete the LiveKit agent course mermaid diagram (per Rob's request above)
<!-- /agent:action-items -->

## Exchange

<!-- agent:exchange patch=append -->
### Re: Granola <-> agent-doc bidirectional

Yes — a bidirectional Granola integration is feasible. The approach:

**Granola → agent-doc (ingest):** Granola exposes shared notes via URL (like the one in this doc). A `corky` adapter or standalone script could poll/webhook Granola notes and hydrate them into agent-doc template documents — exactly what was done manually to create this file.

**agent-doc → Granola (push back):** This is harder since Granola's API surface for *writing* is limited. Two pragmatic paths:
1. **Browser automation** — use the chromium-bridge MCP to paste agent-doc output back into the Granola note
2. **Shared link as read-only mirror** — generate a rendered HTML/markdown view of the agent-doc exchange that Granola users can reference (like a companion artifact)

The most immediate win: automate the ingest side (Granola → agent-doc) so meeting notes flow into template documents without manual copy-paste. The push-back can start as a browser-paste workflow and graduate to API when Granola exposes write endpoints.

### Re: Agent-docs as universal artifact interface

This is the right framing. Agent-doc's core primitive — named components with CRDT merge, patch blocks, and git-backed history — is already source-agnostic. The document template is the universal interface; the *adapter* is what varies per source.

Architecture for multi-source agent-doc:

```
[Granola] ──adapter──┐
[Figma Jam] ─adapter──┤
[Build Party] ─adapter─┼──→ agent-doc template ──→ agent harness (Claude Code / tmux)
[Shimmer] ──adapter───┤                              │
[Any web UI] ─adapter─┘                              └──→ patch response ──→ adapters ──→ sources
```

Each adapter handles:
1. **Ingest**: Transform source artifacts into `&lt;!-- agent:notes --&gt;` component content
2. **Respond**: Route `<!-- patch:* -->` blocks back to the source format
3. **Sync**: Bidirectional conflict resolution (agent-doc's CRDT handles the doc side; adapter handles source-side merging)

The economic advantage: you build the agent harness interaction *once* in agent-doc, then each new source only requires a thin adapter. The harness (diff routing, template patching, git history, session analysis) is shared infrastructure.

For Build Party specifically, this means attendees could interact with agents through Build Party's UI while the agent harness runs agent-doc underneath — the Build Party adapter translates between LiveKit room events and agent-doc patches.

### Re: Build Party — expanded vision

Your annotations sharpen the pitch significantly:

**LiveKit-enabled interactive components**: Polls, agent harness interactions, deep surveys, and hackathons as *composable primitives* that event organizers assemble. The sandboxed agent harness is the key differentiator — it enables collaborative coding without prompt injection risk (the same blocker agent-doc identified for multi-user).

**Sandbox as trust layer**: "If we provide a sandbox, then it safely protects against malicious prompts" — this maps directly to the prompt injection problem. The sandbox boundary means untrusted participants can interact with agents without compromising the session. This is a concrete product feature, not just infrastructure.

**Open questions from your annotations:**
- Collaborative hackathons + security concerns → the sandbox model addresses this, but needs a spec for what "sandboxed agent harness" means in a LiveKit room context (per-participant isolation? shared workspace with access controls?)
- Courses to learn LiveKit within a sandboxed harness → this is a strong go-to-market: LiveKit itself as the first "course provider" on Build Party

### Re: Shimmer — daily blog posts & Cor.e/Trieve

**Daily blog posts from work activity**: This is a compelling Shimmer feature — automated daily digests synthesized from git commits, Claude Code sessions, and connected services. The pipeline:

1. **Collect**: git log, Claude session exports, calendar events, Slack activity
2. **Synthesize**: Agent summarizes themes, decisions, blockers — similar to how Brian's blog post analyzed 24hrs of agent-doc themes
3. **Publish**: Push to a blog/feed that collaborators subscribe to (Rob seeing Miguel's daily output becomes *reading Miguel's daily post*)

This transforms Shimmer from a session viewer into an *activity narrator* — much higher value.

**Cor.e and Mintlify/Trieve:**
- **Cor.e** — persistent memory MCP server. Stores cross-session context as structured observations. Rob and Miguel share a login, meaning they share a memory space. Think of it as the "long-term memory" layer that persists between Claude Code sessions.
- **Trieve** — semantic + full-text hybrid search engine. Was acquired by **Mintlify** (the docs platform). Mintlify uses Trieve to power intelligent documentation search. In Shimmer's context, Trieve would index session content and make it searchable with natural language queries ("when did we discuss the auth middleware rewrite?" → finds the relevant session).
- **Vespa** — the Apache-licensed alternative to Cor.e for the memory layer, with better cross-session capability. The choice between Cor.e and Vespa likely comes down to hosted-vs-self-hosted and how much cross-session intelligence Shimmer needs.
Where would this logic live? Note I'm also looking at an adapter with proof...see tasks/software/proof.md
- Prototype Granola → agent-doc ingest adapter (automate meeting note hydration)

### Re: Where the adapter logic lives + Proof as prior art

The Proof integration (tasks/software/proof.md) already answered this — the same architecture applies to all adapters.

**Each adapter is a Claude Code skill**, not agent-doc core. From the Proof session's conclusion:

> agent-doc is a Rust binary focused on local document lifecycle. Adding HTTP client code, auth, and cloud sync would bloat its single responsibility.

The adapter layer is a family of skills:

| Skill | Source | Pattern |
|---|---|---|
| `/proof` | Proof editor | `sync`/`poll`/`link` — designed, not yet built |
| `/granola` | Granola notes | `ingest`/`push`/`link` — to be designed |
| `/shimmer` | Shimmer sessions | `import`/`export` — future |
| `/buildparty` | Build Party rooms | LiveKit events → patches — future |

Each skill follows the same contract:
1. Calls `agent-doc preflight` and `agent-doc write --stream` for the local side
2. Handles source-specific API calls (Proof REST API, Granola URLs, LiveKit events)
3. Stores link metadata in `.agent-doc/<source>-links.toml`

**The Granola ingest adapter** is the simplest starting point — read-only (Granola → agent-doc), no write-back initially. `/granola ingest <url>` would:
1. Fetch shared note content from the Granola URL
2. Hydrate into an agent-doc template (`&lt;!-- agent:notes --&gt;` component)
3. Create frontmatter and component structure automatically

This is exactly what was done manually to create this file — automating that first step is the prototype.

Let's transfer all the skill + binary stuff over to tasks/software/granola.md
### Re: Transfer to tasks/software/granola.md

Created `tasks/software/granola.md` with:
- Universal adapter architecture (skill-per-source pattern)
- `/granola` skill design (ingest/push/link commands)
- Ingest pipeline spec (fetch → hydrate → template)
- Push-back options (browser automation → API)

This doc stays focused on the meeting notes and Rob relationship. Software implementation moves to the granola.md session.

Related sessions: tasks/software/proof.md (Proof adapter), tasks/software/granola.md (Granola adapter)
<!-- agent:boundary:64f05de2 -->
<!-- /agent:exchange -->

## Pending

<!-- agent:pending patch=replace -->
- Prepare for Sunday follow-up call (5pm ET / 2pm PT, Apr 5)
- Define "sandboxed agent harness" for Build Party LiveKit room context
- Explore Shimmer daily digest pipeline (git + sessions + services → blog post)
- See tasks/software/granola.md for adapter implementation work
<!-- /agent:pending -->
