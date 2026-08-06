---
name: reference_agent_doc_hand_commit_leaves_stale_snapshot_empty_preflight_contract
description: "Hand-committing the session doc with `git commit --only` leaves the agent-doc snapshot AHEAD of HEAD; the next preflight emits its marker with an EMPTY body and the queue silently stops draining."
metadata: 
  node_type: memory
  type: reference
  originSessionId: e02d13b6-2abb-4ec9-baeb-f98355152c7a
  modified: 2026-08-03T03:06:10.514Z
---

Observed 2026-08-02 on `tasks/professional/equityfundingsource.md`.

The documented fallback for a duplicated-response repair ends in
`git commit --only -- <doc>` (see
[[feedback_degraded_ipc_write_retry_duplicates_response]]). That commits HEAD but
does **not** update the agent-doc snapshot, so the snapshot is left *ahead* of
HEAD — here `snapshot_len=43434` (still holding the duplicate) vs
`head_len=40203` (deduped).

**The failure is silent, and that is what makes it expensive.** The next turn's
`UserPromptSubmit` hook still prints the `[agent-doc] cycle contract ...` marker,
so it does not look like the fail-closed "missing marker" case — but the JSON body
after it is **empty**. No `no_changes`, no `queue_prompts`, no tier fields.
`agent-doc plan` agrees: `prompt_targets`, `repo_actions` and `pending_mutations`
are all absent while `execution_scope` still says `normal`. An active `go` queue
with unstruck heads simply never gets selected, and the turn does nothing.

Because preflight computes its prompt by diffing the document against the
snapshot, a snapshot ahead of HEAD makes the only visible "diff" the removal of
the stale content — never a queue-head prompt line — so no head is chosen.

**Fix (worked here in one command):**

```
agent-doc reset --from-current --preserve-session <FILE>
agent-doc session-check <FILE>     # -> ok, committed; queue_continuation_required=true
```

This is the opposite of [[feedback_snapshot_vs_head_guard_stash_reset_to_head]],
where `reset --from-current` does NOT fix it and a stash dance is required. The
distinguishing condition: here the **working tree already equals HEAD** (the
dedup was committed), so `--from-current` rebuilds snapshot == HEAD, which is
exactly what the diff logic wants. When the working tree carries an uncommitted
user edit, `--from-current` sets snapshot == working-tree (still not HEAD) and the
guard persists — that is when you need the stash sequence.

**`reset --from-current` deferring once is not a failure.** The first attempt
returned `waiting for automatic editor-authority convergence ... no operator save,
reload, or retry is required` with a hash mismatch. A retry a few commands later
succeeded outright. Do not escalate to `--force-disk` on the first deferral.

Diagnose with `session-check`, which names it exactly: `cycle state is committed
but the snapshot does not match HEAD in the owning repo (snapshot_len=...,
head_len=...)`. Ignore its `[missing_response_body]` recovery hint here — piping a
response body back through `write --commit` is what duplicates responses in the
first place.
