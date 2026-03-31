---
agent_doc_session: 4ef1150a-7b51-4ff6-9284-9c7c1f1570e5
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
Linked with bob-remeika.md | Web prototype MVP scoped | Ragie Parse use cases: Judith (loans), Ron (patents)
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Re: New session — ragie-web-doc

Fresh document with empty components. What's the goal here? Add your prompt to the exchange component and I'll pick it up on the next cycle.

Link this document with tasks/professional/bob-remeika.md
### Re: Linking with bob-remeika.md (HEAD)

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
<!-- agent:boundary:f5bb5f13 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Build web prototype MVP (markdown editor + Ragie retrieval + Claude responses)
- [ ] Test VS Code agent-doc extension
- [ ] Index demo documents in Ragie (patent proposal, contacts, resume)
- [ ] Prepare demo script for Bob call
- [ ] Explore Ragie Parse for Judith's loan docs and Ron's patent files
<!-- /agent:pending -->
