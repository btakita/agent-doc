---
agent_doc_session: b70be1e0-5f53-420d-862d-07f50661c114
agent_doc_format: template
agent_doc_write: crdt
---


## Meeting: Brian Takita / Rob Ross

**Date:** April 2, 2026
**Author:** Robert Ross (via Granola)
**Source:** [Granola shared note](https://notes.granola.ai/d/e4d8dd00-6468-4a0b-93f1-df1f7d84d956)

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
- Watch party, pre/post-party, co-build rooms, competitions (vibe coding hackathons), courseware
- Built on LiveKit; targeting tool providers (e.g., LiveKit) as first strategic partners
- LiveKit raised $100M in Jan 2026; Neil Dwyer (built LiveKit agent framework) is an advisor

**Shimmer:** developer sidekick/helper app
- View, back up, and semantically search Claude Code sessions and assets
- Share sessions with collaborators (e.g., Rob seeing Miguel's daily output)
- Memory layer via Cor.e or Vespa; semantic search via Trieve (acquired by Mintlify)
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

<!-- /agent:exchange -->

## Pending (HEAD)

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
