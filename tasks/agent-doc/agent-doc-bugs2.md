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
### Re: uncommitted response patch in claudescore.md

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

❯ We had 2 sessions running the same agent doc?

- Session committed snapshot with just `done (HEAD)` (clean state)
- A second pane (stash) started a new response cycle on claudescore.md
- That session wrote the streaming checkpoint but was interrupted before the final write
- The checkpoint remains as uncommitted working-tree content
- The new boundary hash (`cabc81e1`) replaced the committed one (`0f29e90b`), removing `(HEAD)` from `done`

**The fix needed in agent-doc:**

When a new preflight detects that the working tree has content added *between* a committed `(HEAD)` boundary and a new boundary, it should offer to discard the orphaned intermediate content. Currently `preflight recover` only handles the case where a full response was written but not committed — it doesn't detect partial streaming checkpoints that precede a committed final response.

**Immediate action for this file:**

The intermediate checkpoint block (lines 1504–1522) can be removed — it's superseded by the `done` response. This is a manual cleanup since the automated recovery path doesn't cover this case yet.

❯ do it
### Re: two sessions / do it

**"We had 2 sessions running the same agent doc?"** — Yes. The stash panes (`%34`/`%36`) each held an `agent-doc` session. When both pointed to `claudescore.md`, the second session's preflight diffed the file against the last committed snapshot and saw the `done` response as "already committed" baseline, then started its own response cycle — racing the first session's in-flight streaming checkpoint write.

**Done.** Removed the orphaned checkpoint block from `src/session-share/tasks/claudescore.md` and committed in the submodule:

```
chore(claudescore): remove orphaned streaming checkpoint
Removes the intermediate `### Re: local Postgres setup` block that was
left behind by an interrupted second agent-doc session (race condition
between two stash panes pointing at the same document).
```

Remaining diff (boundary hash `0f29e90b` → `cabc81e1` + `(HEAD)` removal) reflects the second session taking over document ownership — that's now committed too.

❯ How did we get an orphaned tmux pane? Can we have a safeguard to prevent this from happening?
❯ eIdeally multiple safeguards?
### Re: orphaned pane + safeguard

**How the stash pane formed:**

The stash window is a parking zone. When `route.rs` can't split the main window (layout full, or window exists but no registered pane), it moves the Claude pane there rather than creating a new window. That stashed pane is still a *registered* session — it just lives in `stash`. It can still trigger response cycles on its claimed document.

**The gap: no document-level uniqueness constraint.**

The binding invariant (`claim.rs:162–186`) prevents pane theft (a pane can't be commandeered by another doc), but it doesn't prevent **two panes from registering to the same document**. Both panes see the same committed snapshot as baseline, both run preflight, both start response cycles → race condition.

**Existing mitigations that don't cover this:**
- `route.rs:621–626` — 5s startup lock at `.agent-doc/starting/<hash>.lock` — only blocks double-spawn on quick re-trigger, not concurrent long-running sessions
- `preflight.rs:227–267` — stash-pane detection is **informational only**, preflight continues anyway

**Safeguard options:**

1. **Enforce one pane per document in `sessions.json`** (cheapest). On `register_with_pid()`, if another live pane already claims this document's canonical path, reject or evict it. Single enforcement point, uses existing `RegistryLock`.

2. **Extend startup lock to span the full response cycle.** Preflight acquires `.agent-doc/starting/<hash>.lock`, holds it until `agent-doc commit` releases it. Overlapping preflight aborts immediately.

3. **Preflight hard-block on stash conflict.** Promote the current warning to an error when the stash pane's claimed document matches the current one — abort that cycle.

**Recommendation:** Option 1 + Option 3. Option 1 makes the invariant explicit at registration time. Option 3 is a cheap belt-and-suspenders check that catches edge cases where the registry gets stale.
### Re: multiple safeguards (HEAD)

Yes — all three options are independent layers and all should be implemented:

**Layer 1 — Registration (Option 1):** Document-path uniqueness in `sessions.json`. Prevents two panes from ever both being registered to the same document. Root fix — blocks the race at its origin.

**Layer 2 — Preflight entry (Option 3):** Hard-block when a stash pane's claimed document matches the current document. Catches cases where the registry is stale or bypassed.

**Layer 3 — Active cycle lock (Option 2):** Extend the startup lock to span the full response cycle. Even if Layers 1+2 fail (registry corruption, concurrent claim race), no two preflight runs can proceed simultaneously for the same document.

**Layer 4 — Recovery (existing gap):** `preflight recover` should detect orphaned streaming checkpoints that sit between a committed `(HEAD)` boundary and a new boundary, and auto-discard them. Closes the cleanup gap when the race slips through all other layers.

Each layer catches a different failure mode:

| Layer | What it blocks |
|-------|---------------|
| 1 — Registry uniqueness | Two panes registered to same doc at claim time |
| 2 — Preflight stash check | Second pane triggering a cycle while first is active |
| 3 — Cycle lock | Concurrent preflight runs even with stale/bypassed registry |
| 4 — Recovery | Cleanup of leftover partial writes after a race |
<!-- agent:boundary:f7aac7c2 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Layer 1: Enforce doc-path uniqueness in `sessions.json` at `register_with_pid()` — evict stale same-doc registrations
- [ ] Layer 2: Promote stash-pane conflict from warning → error in preflight (abort cycle when stash pane owns same doc)
- [ ] Layer 3: Extend `.agent-doc/starting/<hash>.lock` TTL to span full response cycle (release on `agent-doc commit`)
- [ ] Layer 4: `preflight recover` — detect orphaned checkpoint blocks between committed `(HEAD)` and new boundary; auto-discard
<!-- /agent:pending -->
