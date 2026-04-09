---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
❯ Why does src/session-share/tasks/claudescore.md have an uncommitted response patch?
### Re: uncommitted response patch in claudescore.md (HEAD)

**Root cause: streaming checkpoint stuck after interrupted/overlapping session.**

The file has two back-to-back response headers for the same prompt (`❯ Please do the local postgres setup for me.`):

1. **Lines 1504–1522** — `### Re: local Postgres setup` — streaming checkpoint ("Running. `supabase start` is pulling images...")
2. **Lines 1523–1548** — `### Re: local Postgres setup — done` — final response with full status

The snapshot only contains `### Re: local Postgres setup — done (HEAD)` — the checkpoint is absent from it.

**How this happens with `patch=append` exchange components:**

1. Checkpoint write → appends `### Re: local Postgres setup` + partial content
2. Final write → appends `### Re: local Postgres setup — done` + full content  
3. CRDT merge treats both as new appended content — no de-duplication

**Why the snapshot diverged:**

The preflight also flagged two `agent-doc` panes running in the stash window (`%34`, `%36`). The likely sequence:

- Session committed snapshot with just `done (HEAD)` (clean state)
- A second pane (stash) started a new response cycle on claudescore.md
- That session wrote the streaming checkpoint but was interrupted before the final write
- The checkpoint remains as uncommitted working-tree content
- The new boundary hash (`cabc81e1`) replaced the committed one (`0f29e90b`), removing `(HEAD)` from `done`

**The fix needed in agent-doc:**

When a new preflight detects that the working tree has content added *between* a committed `(HEAD)` boundary and a new boundary, it should offer to discard the orphaned intermediate content. Currently `preflight recover` only handles the case where a full response was written but not committed — it doesn't detect partial streaming checkpoints that precede a committed final response.

**Immediate action for this file:**

The intermediate checkpoint block (lines 1504–1522) can be removed — it's superseded by the `done` response. This is a manual cleanup since the automated recovery path doesn't cover this case yet.
<!-- agent:boundary:93d0c788 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
