# Plan: graceful document-drift resolution (no agent-visible thrash)

**Backlog id:** `#docdriftgrace`
**Status:** TBD (not started)
**Filed:** 2026-06-18 (agent-doc-bugs2.md dogfooding session)

## Problem

agent-doc thrashes when the session document changes during/between a turn in a
way that is *unrelated to the current prompt/response*. The binary surfaces the
drift as a blocking conflict that the **agent** must resolve by hand instead of
the **binary** resolving it silently.

### Live repro (this session)

The visible file had two old committed `### Re:` response sections trimmed by the
operator, the harness switched `agent: opencode → claude`, and a new queue prompt
was added. Preflight then refused to commit:

```
[preflight] interrupted-cycle commit warning: refusing commit ...: snapshot is
41434 bytes but the visible file is 36814 bytes, which looks like a manual
cleanup with stale snapshot/CRDT state. Reset the sidecars from the current file
before writing: `agent-doc reset --from-current <FILE>`
```

`committed: false` persisted through preflight's own recovery attempt; the agent
had to manually run `agent-doc reset --from-current --preserve-session <FILE>`
and re-run preflight to proceed. None of the changes touched the in-flight
prompt — they were ordinary exchange trimming + frontmatter + queue bookkeeping.

## Desired behavior (operator statement)

> agent-doc should not thrash when the document changes during a turn ... when
> the current document does not match the snapshot, the binary should gracefully
> resolve the difference without the agent knowing about it. Only structural
> conflicts based on the current work should be surfaced ... such as editing the
> current prompt. Unrelated changes should be gracefully handled by the
> agent-doc binary without the agent needing to intervene.

## Fix direction (do not implement without operator go + live re-baseline)

1. **Classify the drift, don't size-gate it.** The current `snapshot > visible`
   "looks like a manual cleanup" heuristic (in `git.rs` / preflight commit guard)
   keys off byte-size delta. Replace/augment with a *component-scoped* classifier
   that asks **which components changed**:
   - Trimmed/edited **historical** `exchange` `### Re:` blocks (already committed
     to HEAD), `frontmatter` `agent:`/model switches, `queue`/`backlog`/`review`
     bookkeeping → **unrelated**: auto-rebase the snapshot/CRDT onto the visible
     file silently (the equivalent of `reset --from-current --preserve-session`),
     log it at info level, do **not** block.
   - Edits to the **current in-flight prompt** (the `exchange` tail prompt being
     answered this cycle) or the response block being written → **structural
     conflict on current work**: surface as today.
2. **Make preflight's own recovery actually resolve case (1).** Preflight already
   "attempts recovery before diff" but here it could not — it fell through to the
   blocking warning. The auto-rebase for unrelated drift should run *inside*
   preflight so `committed: true` is reached without agent intervention.
3. **Guard against re-injecting trimmed content.** The existing `commit`
   auto-recover path (`live_prompt_drift_auto_recovered`) re-adds snapshot content
   that is larger than visible — which is exactly wrong when the operator
   *intentionally* trimmed. The classifier must treat "HEAD already contains the
   trimmed blocks + visible is a clean superset-minus-old-responses" as an
   intentional trim, not a response loss. (Contrast with the genuine
   response-loss case where the snapshot holds an *uncommitted* full response.)

## Risk / sequencing

- Live-supervisor-critical and drift-classifier-critical: a wrong classification
  either (a) re-injects content the operator deleted, or (b) silently drops a
  real current-prompt conflict. Needs SimWorld coverage for **both** the
  intentional-trim and genuine-response-loss cases, plus `make check` + a focused
  operator-watched live re-baseline (drive one trim-during-turn cycle and confirm
  no blocking warning + no content re-injection).
- Related committed invariants to keep aligned: the `git.rs` selective-commit
  "manual cleanup with stale snapshot" guard, `preflight.rs` recovery ordering,
  `capture.rs` replay-hash validation (response-loss path), and the SKILL/runbook
  recovery guidance (`reset --from-current` becomes a fallback, not the routine
  path).

## Related backlog / memory

- `#routedrainnextaction` (DONE this session) — a narrower instance: a stuck
  cycle surfaced a misleading next_action instead of the recovery command.
- The agent-loop memory has multiple drift/recovery feedback notes
  (`compaction_stale_snapshot_reset_not_commit`,
  `ipc_crdt_response_loss_forcedisk_recovery`,
  `supervisor_crdt_replay_reset_from_current`) — this plan aims to make most of
  those agent-side recovery rituals unnecessary by moving resolution into the
  binary.
