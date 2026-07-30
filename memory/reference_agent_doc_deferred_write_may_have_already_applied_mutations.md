---
name: reference_agent_doc_deferred_write_may_have_already_applied_mutations
description: "`agent-doc write --commit` reporting `visible document write deferred: authority_unavailable` may have ALREADY applied its backlog/queue mutations to disk — inspect the document before retrying, or the retry dies on the duplicate-id guard"
metadata:
  node_type: memory
  type: reference
---

Seen 2026-07-30 on `tasks/software/lazily.md` (agent-doc 0.35.99).

`agent-doc write --commit <FILE> --backlog-only --done ... --backlog-add ...`
exits non-zero with:

```
Error: visible document write for <FILE> deferred: Lazily current transition
remained authority_unavailable for 8000ms; retry after it settles
```

[[reference_agent_doc_retained_pending_write_superseded_hash_agent_doc_commit]]
says to retry rather than escalate on `authority_unavailable`, and that is right —
but **the deferral is about the visible/commit step, not the mutation step.** The
`--done` strikes, queue strikes, and `--backlog-add` items were already written to
the working tree. A blind retry loop then fails on a *different* error that looks
like a new problem:

```
Error: pending add: refusing to add item with explicit id `<id>` — that identity
is already active under agent:backlog (preset-item-id-collision-enforce)
```

**Before retrying a deferred write, read the document.** `grep -oE '^- \[.\] \[#[a-z0-9]+\]' <FILE>`
and the `## Queue` block tell you whether the mutations landed. If they did, there
is nothing to re-apply — only the commit is outstanding, and the controller owns
it. Wait on `agent-doc session-check <FILE>` in an `until` loop instead.

Related but distinct: in the same cycle, `agent-doc respond` reported
`serialized_atomic_write: ... remains retained after the foreground editor ACK
deadline`. There the **response body** committed (via controller
`session-check-recovery` → `post_commit_reposition_crdt_relay_acknowledged`) but
the tracked-work mutations were silently dropped — `finalize` had logged
`cycle ... was already committed; reopened a fresh cycle for a pending-only
tracked-work update` (the committed-wedge path). So a `session-check` that returns
`ok — cycle ... is committed` does **not** prove the `--done` / `--backlog-add`
flags took effect. Verify the backlog and queue text, and re-apply with
`write --commit --backlog-only` if they are missing.

Both paths deferred with `controller_model_backpressure` first
(`cooldown_secs=30`), so budget a minute or two per attempt and never
`--force-disk`. [[reference_agent_doc_recovery_index]]
