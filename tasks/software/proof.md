---
agent_doc_session: dcce73b6-5057-4f40-b37a-f80cde8d03b4
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
Proof integration: Recommended Claude Code skill (`/proof`) over agent-doc core. Designed comment overlay mechanism.
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
### Re: Proof Investigation

**Proof** is a collaborative document editor by [Every](https://every.to) designed for human-AI co-authoring. Key features:

- **Provenance tracking** — colored gutter shows who wrote what (green = human, purple = AI)
- **Suggestions** — AI edits create track-changes style suggestions (accept/reject)
- **Comments** — threaded comments with replies, anchored to specific text
- **Agent API** — full REST API for reading, editing, commenting, and presence

I've successfully joined the document at `mu0anyag` and left a comment confirming my presence. You should see "Claude (agent-doc)" in the presence list.

### Re: agent-doc Integration Possibilities

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

I prefer Option 1 to option 2.

2. **Proof as an input source** — agent-doc could watch a Proof doc's event queue (`/events/pending`) and treat edits as user input, responding via the ops API.

3. **Bridge skill** — a `/proof` Claude Code skill that creates Proof docs from local markdown, syncs bidirectionally, and lets agent-doc sessions be shared without exposing the local filesystem.

Explain...Does this mean the other options would allow the proof editor to access the local file system?
> lets agent-doc sessions be shared without exposing the local filesystem

4. **Comment threading** — Proof's anchored comments could enrich agent-doc's annotation model, which currently uses inline markdown comments.

Compare Proof's anchored comments with agent-docs's inline markdown comments

The API is well-designed — token auth, CRDT-aware mutations with base tokens, idempotency support. The `edit/v2` endpoint with block references is particularly clean for programmatic editing.

What direction interests you most?
update from proof
### Re: "update from proof"

I saw you wrote "Tell me about proof." in the Proof document. I responded there directly via the edit/v2 API — you should see my response with purple provenance gutter in the Proof editor.

The round-trip worked:
1. You wrote in Proof ("Tell me about proof.")
2. You signaled agent-doc with "update from proof"
3. I read the Proof doc state, saw your question, and responded via edit/v2
4. Provenance marks confirm authorship: `human:Brian` for your text, `ai:claude-agent-doc` for mine

This validates integration angle #2 (Proof as input source) — the manual bridge works. An automated version would poll `/events/pending` or `/queue/pending` and route changes back through agent-doc's diff pipeline.

**API learnings from this round-trip:**
- `edit/v2` uses `operations` array (not `ops`), with `blocks` array per operation
- Each block is `{"markdown": "..."}` — Proof handles parsing into its internal format
- Presence API requires `X-Agent-Id` header
- Provenance is automatic — the `by: "ai:claude-agent-doc"` attribution flows through
### Re: Option 1 preferred over Option 2

Noted. Option 1 (Proof as a remote surface) makes more sense as the primary direction — agent-doc stays the source of truth while Proof provides a shareable view with provenance. Option 2 (Proof as input source) is still useful as a complement but secondary.

Yes...so how do you recommend we package this? Should this be part of agent-doc or a plugin to agent-doc?

### Re: Filesystem exposure clarification

No — none of the options expose your local filesystem to Proof. The distinction is about **what gets shared**:

- **Options 1, 2, 4** — the agent (Claude) acts as an intermediary. It reads local files via agent-doc and writes to Proof via API. Proof never sees your filesystem. The content flows through Claude's API calls.
- **Option 3 (Bridge skill)** — same pattern, just more automated. The phrase "without exposing the local filesystem" was emphasizing that the skill would selectively sync content rather than mounting or linking files. It was poorly worded — all options keep Proof sandboxed to its cloud API. The local filesystem is never accessible to Proof in any option.

The key architectural guarantee: Proof only receives what Claude explicitly sends via HTTP. There's no filesystem mount, no file path sharing, no direct access.

### Re: Anchored comments vs inline markdown comments

| | Proof (anchored) | agent-doc (inline markdown) |
|---|---|---|
| **Anchor mechanism** | Quote-based text matching — `{"anchor": {"quote": "target text"}}` | Position-based — comments sit between content lines |
| **Persistence** | Stored in `marks` metadata, separate from content | Embedded in the markdown file itself |
| **Threading** | Native — `comment.add` creates thread, `comment.reply` adds to it via `markId` | No threading — comments are flat `[//]: #` or `<!-- -->` blocks |
| **Resolution** | `comment.resolve` / `comment.unresolve` API ops | Manual deletion by the user |
| **Visibility** | Rendered as sidebar annotations in Proof UI | Stripped by agent-doc's diff engine — invisible to the agent unless explicitly read |
| **Authorship** | `by: "ai:agent-name"` or `by: "human:Name"` | No attribution — all comments look the same |
| **Drift resilience** | Quote anchors can break if text is heavily edited | Position-based, so they survive in-place edits but can be displaced by insertions |
| **Agent interaction** | Agent can read all marks, reply, resolve | Agent explicitly ignores comments (by design — comments are user scratchpad) |

**Key difference in philosophy:** Proof comments are a collaboration tool — agents read and respond to them. agent-doc comments are a privacy tool — they're explicitly excluded from the diff so users can keep private notes the agent never sees.

For integration, Proof's anchored comments could serve as the "public discussion" layer while agent-doc's inline comments remain the "private scratchpad." This would give users both modes in one workflow.
How would proof comments be overlaid in this agent doc?
### Re: Packaging — agent-doc core vs plugin

I'd recommend a **Claude Code skill** (not baked into agent-doc core). Here's why:

**Why a skill, not agent-doc core:**
- agent-doc is a Rust binary focused on local document lifecycle (snapshots, diffs, CRDT writes, git integration). Adding HTTP client code, Proof auth, and cloud sync would bloat its single responsibility.
- The sync logic is inherently agent-mediated — Claude reads the local doc and writes to Proof's API. agent-doc doesn't need to know about Proof; Claude does.
- A skill can evolve independently of agent-doc releases. Proof's API could change without requiring a new agent-doc binary.

**What the `/proof` skill would do:**
1. `proof sync <file>` — push current agent-doc exchange to a Proof doc (create or update)
2. `proof poll <file>` — check for Proof edits since last sync, inject into agent-doc exchange
3. `proof link <file> <slug>` — associate a local file with a Proof doc (stored in `.agent-doc/proof-links.toml`)
4. `proof unlink <file>` — remove the association

**What stays in agent-doc:**
- Nothing changes. agent-doc continues doing local snapshots, diffs, CRDT writes, git commits.
- The skill calls `agent-doc write` and `agent-doc preflight` just like the current `/agent-doc` skill does.

**What the skill needs from agent-doc (already exists):**
- `agent-doc preflight` — get current doc state and diff
- `agent-doc write --stream` — write back merged content
- Snapshot files — to track what's been synced

This is essentially the same pattern as the `/agent-doc` skill itself — a Claude Code skill that orchestrates agent-doc CLI commands with external service calls.

### Re: Proof comment overlay in agent-doc

The overlay would work through the **sync skill**, not by modifying agent-doc's format. Here's the concrete mechanism:

**On `proof sync` (push to Proof):**
- Skill reads the agent-doc exchange component
- Maps agent-doc content blocks to Proof blocks via `edit/v2`
- Proof comments stay in Proof's `marks` metadata — they don't exist locally

**On `proof poll` (pull from Proof):**
- Skill reads `GET /api/agent/<slug>/state` and checks `marks` for new/updated comments
- New Proof comments get injected into the agent-doc exchange as a special annotation block:

```markdown
> **[Proof comment by Brian on "target text"]:**
> This needs more detail about error handling.
> 
> **[Reply by Claude]:**
> Added error handling section below.
```

- These annotations appear in the agent-doc diff on the next `/agent-doc` cycle, so Claude can respond to them
- The skill tracks which Proof `markId`s have been synced to avoid duplicates (stored in `.agent-doc/proof-links.toml`)

**On agent-doc response:**
- If Claude's response addresses a Proof comment, the skill can call `comment.reply` or `comment.resolve` on the Proof side

**What this looks like in practice:**
1. Collaborator comments on text in Proof UI
2. You run `/proof poll proof.md` — comment appears as annotation in your local doc
3. You run `/agent-doc proof.md` — Claude sees the annotation, responds
4. You run `/proof sync proof.md` — response + comment reply pushed to Proof

The key design choice: Proof comments are **transient imports** in agent-doc, not permanent storage. The source of truth for comments stays in Proof's marks system. The local annotations are just a bridge for the agent-doc diff cycle.
<!-- agent:boundary:ca42737b -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- **Active:** Design `/proof` Claude Code skill for Option 1 (remote surface)
- Skill commands: `sync`, `poll`, `link`, `unlink`
- Comment overlay: transient imports from Proof marks → agent-doc annotations
- Config storage: `.agent-doc/proof-links.toml`
- Next step: prototype `/proof` skill SKILL.md and implement `proof sync`
<!-- /agent:pending -->
