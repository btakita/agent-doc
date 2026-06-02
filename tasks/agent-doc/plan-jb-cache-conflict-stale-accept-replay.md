# Plan: Recover JB File Cache Conflict accepted late (stale response replay)

**Status:** In progress
**Created:** 2026-06-02
**Session:** `tasks/agent-doc/agent-doc-bugs2.md`
**Backlog id:** `#jb-cache-conflict-stale-accept-replay`

## Symptom (live repro)

After a Phase 6 (`#lr-content-6`) cycle committed correctly to `HEAD`, the commit
step queued an IPC reposition patch for the IDE
(`[commit] file IPC reposition patch queued: .agent-doc/patches/<hash>.json`).
That patch carried an **earlier draft** of the same response — the body from the
first `finalize --stream` attempt that had timed out on IPC (it still contained a
"Note: the orphaned pre-compaction stuck_captured_cycle …" paragraph that the
successful `--force-disk` retry had dropped).

Hours later the user opened the document in JetBrains and accepted the **File
Cache Conflict** dialog, which replayed that stale queued patch into the buffer.
Disk became `HEAD` **plus a surplus duplicate** `### Re: #lr-content-6 …` block
(marked `(HEAD)`, with the stale body), inserted before the exchange boundary.

`session-check` then misclassified the drift as a **direct response patchback**
(`detect_bypassed_response_write`) and told the user to run
`agent-doc write --commit`, which would have committed a duplicated/stale body.

## Root cause

The existing late-replay recovery only recognizes a duplicate whose body is
**byte-identical** to a committed response:

- `dedupe::is_committed_response_overapplication(current, head)` requires the
  response *multiset* of `current` to equal `head`'s plus extra exact copies
  (`cur_set == head_set`). A **stale** replay (an earlier/edited draft of the
  same response) has a different body, so `cur_set != head_set` → returns false.
- `dedupe::dedupe_responses` / `first_duplicate_response_heading` only collapse
  **consecutive, byte-identical** `### Re:` blocks; the stale twin differs.
- `detect_jb_cache_conflict_accept_duplicate_replay` depends on the dedupe path,
  so it also misses the stale case.

All three fall through to the generic direct-patchback guard.

## Fix

A surplus working-tree response block whose `### Re:` heading **topic** already
appears in a committed `HEAD` response is a replay artifact regardless of body
drift — an agent never legitimately writes two responses with the same heading
topic, and `HEAD` is the authoritative committed version. Restoring `HEAD` is
provably safe as long as every committed response is still present unchanged.

1. `dedupe::is_committed_response_replay_including_stale(current, head)` — a
   heading-topic-tolerant superset of `is_committed_response_overapplication`:
   scaffold must match, every committed response body must still be present, and
   every surplus block must duplicate an already-committed heading topic (the
   ` (HEAD)` suffix is normalized away). No committed response may be lost or
   altered, and there must be at least one surplus block.
2. `session_check::detect_late_ipc_response_overapplication` falls back to the
   new check when the strict check is false. It already returns a remediation
   that `preflight` applies by restoring `HEAD` over the working tree + snapshot
   (and clearing the stale patch), so both the `inspect_core` INTERRUPTED path
   and the preflight auto-repair path now cover the stale replay.

## Verification

- `dedupe` regressions: stale-body surplus block sharing a committed topic is
  recognized; a genuinely new topic, a lost/altered committed response, or pure
  scaffold drift are NOT (fail closed to the patchback guard).
- `preflight` regression: a doc shaped like the live repro auto-repairs to HEAD.
- `make check` green; both binaries rebuilt.
- Dogfood: run the rebuilt `agent-doc preflight` on the live corrupted
  `agent-doc-bugs2.md` and confirm it collapses to the committed HEAD.
