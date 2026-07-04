---
name: feedback_qheadresidue_multiline_codeblock_heads_unstrikable
description: "multi-line/code-block free-text queue heads can't be struck by"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: e6c21fed-7fee-479d-80fc-aa04fab4b989
---

`session-check INTERRUPTED: completed free-text agent:queue head(s) "…" are still active in the committed queue even though exchange history contains a Queue prompt echo … (#qheadresidue)` for a **multi-line head that embeds code fences** (e.g. the JB `Run Agent Doc` codex→opencode actor-switch report with route-error code blocks, or a binary-authored `ipc_proof_insufficient` dogfood log) CANNOT be cleared by the normal tools:
- `#ftstrike` only consumes a head when the response quotes the **exact** queue prompt; a single-line `> **Queue prompt:** …` echo (with or without the `:pushpin:` prefix) does NOT match a head whose stored text includes the full code blocks. A single-line console-fragment head DOES strike on a content prefix — the difference is the code-block body.
- `queue prune-noise` will not classify these as noise (the IPC dogfood logs and the actor-switch report don't trip the no-id/no-question/no-verb predicate), and one run wrongly struck an `[operator-verify]` `do [#nsfz]` queue mirror.
- `queue consume` only strikes the **leading** free-text head answered **this turn**; after the cycle closes it no longer sees prior-turn echoes as answered.

**Why:** the route-owned supervisor is in the stale-CRDT churn state (the recorded `queue_pause_reason`) — it re-races every finalize (`queue consume: failed to write document … retry after typing stops`), and queue-strike writes land in the snapshot but never commit to HEAD. Repeated `git checkout HEAD` after each strike command silently reverts the strikes from the working tree.

**How to apply:** do NOT churn strike-commands or Edit the queue (it races+corrupts). Commit the genuine answer (it reaches HEAD fine), `git checkout HEAD -- FILE` + `reset --from-current --preserve-session` to stop snapshot drift compounding, then STOP and report: the residue needs the operator's `agent-doc session restart-supervisor` to clear the stale CRDT + remove the un-strikable rows. The `<!-- no-free-text-queue-head-guard -->` comment only suppresses the interruption (marks the rows intentionally-active) — not a real clear. See [[feedback_degraded_ipc_blocks_granular_pending_writes]], [[feedback_queue_consume_race_forcedisk_recovery]], [[feedback_operator_drive_items_on_go_queue_churn]].
