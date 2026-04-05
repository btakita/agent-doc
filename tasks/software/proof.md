---
agent_doc_session: dcce73b6-5057-4f40-b37a-f80cde8d03b4
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
Connected to Proof doc `mu0anyag`. Comment posted. Investigating integration paths.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Please investigate https://proofeditor.ai/

Can agent-doc integrate with proof?

Proof is a collaborative document editor with presence, comments, suggestions, and edit APIs.

Join this doc immediately so I can see your presence:
https://proofeditor.ai/d/mu0anyag?token=7e6e9d3d-87c8-4e05-bf4d-89e7c029ea77

1. Announce your presence in this doc first.
   If you already know Proof, do that immediately.
   If not, use the tokenized URL above or POST https://proofeditor.ai/api/agent/mu0anyag/presence with the bearer token from the URL.

2. Once you have joined, read the current doc and reply: Connected in Proof and ready.
   Read via GET https://proofeditor.ai/api/agent/mu0anyag/state with the bearer token from the URL, or open: https://proofeditor.ai/d/mu0anyag?token=7e6e9d3d-87c8-4e05-bf4d-89e7c029ea77

3. If you need setup details for deeper interaction, fetch:
   Skill: https://proofeditor.ai/proof.SKILL.md
   Docs: https://proofeditor.ai/agent-docs

4. If Proof looks wrong or the API fails in a surprising way, call:
   POST https://proofeditor.ai/api/bridge/report_bug
   Include a short summary, free-form context, and raw evidence like request/response pairs or request IDs.
   Optional reference code: https://github.com/EveryInc/proof-sdk
### Re: Proof Investigation (HEAD)

**Proof** is a collaborative document editor by [Every](https://every.to) designed for human-AI co-authoring. Key features:

- **Provenance tracking** — colored gutter shows who wrote what (green = human, purple = AI)
- **Suggestions** — AI edits create track-changes style suggestions (accept/reject)
- **Comments** — threaded comments with replies, anchored to specific text
- **Agent API** — full REST API for reading, editing, commenting, and presence

I've successfully joined the document at `mu0anyag` and left a comment confirming my presence. You should see "Claude (agent-doc)" in the presence list.

### Re: agent-doc Integration Possibilities (HEAD)

The two systems are complementary but architecturally different:

| | agent-doc | Proof |
|---|---|---|
| **Storage** | Local markdown files + git | Cloud-hosted CRDT |
| **Collaboration** | Git gutter (committed vs uncommitted) | Live presence + provenance |
| **Edit model** | Append/patch components, snapshots | Block-level edit/v2, suggestions, rewrites |
| **Comments** | Markdown comments (stripped by diff) | Anchored thread system |
| **Auth** | Local filesystem | Bearer token per shared link |

**Integration angles:**

1. **Proof as a remote surface for agent-doc sessions** — agent-doc could sync its exchange component to a Proof doc, giving collaborators a live view with provenance tracking. The edit/v2 API maps well to agent-doc's patch blocks.

2. **Proof as an input source** — agent-doc could watch a Proof doc's event queue (`/events/pending`) and treat edits as user input, responding via the ops API.

3. **Bridge skill** — a `/proof` Claude Code skill that creates Proof docs from local markdown, syncs bidirectionally, and lets agent-doc sessions be shared without exposing the local filesystem.

4. **Comment threading** — Proof's anchored comments could enrich agent-doc's annotation model, which currently uses inline markdown comments.

The API is well-designed — token auth, CRDT-aware mutations with base tokens, idempotency support. The `edit/v2` endpoint with block references is particularly clean for programmatic editing.

What direction interests you most?
<!-- agent:boundary:c095367d -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- Proof integration direction TBD (awaiting user preference)
- Potential: bridge skill, remote surface sync, event-driven input, comment enrichment
<!-- /agent:pending -->
