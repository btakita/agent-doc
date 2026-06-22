# Plan — Compact/commit blocked by `typed_component_drift` auto-adopt false-positive (#compactdrift)

## Symptom (live repro, 2026-06-12/13, agent-doc-bugs2.md)

`JB Compact Exchange` on `tasks/agent-doc/agent-doc-bugs2.md` fails after doing the
archival work:

```
[compact] CRDT state compacted: 26692 → 26692 bytes
[compact] CRDT state refreshed from post-compact content
[compact] discarded 1 stale capture sidecar(s) ...
[compact] Archived 148 lines from component 'exchange' to .agent-doc/archives/...-20260613-022946.md
Error: refusing to auto-adopt committed historical response patchback for
tasks/agent-doc/agent-doc-bugs2.md: HEAD contains an out-of-band
typed_component_drift mutation with response marker
### Re: do [#rtwbcast] — multi-editor CRDT broadcast — opus-4-8
```

The **same guard** blocks plain `agent-doc commit` recovery (observed at this
session's start: identical `refusing to auto-adopt committed historical response
patchback … out-of-band typed_component_drift mutation with response marker
### Re: do [#rtwbcast]`). Workaround used to recover: kill the wedged
`--handoff-state preparing` controller, `reset --from-current --preserve-session`,
then `git checkout HEAD`.

## Root-cause hypothesis

The auto-adopt guard that protects against committing unreviewed `typed_component_drift`
(out-of-band typed-component mutation) is keying on the **presence of a committed
`### Re: …` response marker in HEAD** as evidence of out-of-band drift. But a
legitimately-committed historical response (the normal post-finalize HEAD state)
ALSO has that marker — so the guard cannot distinguish:

- (a) genuine out-of-band drift: a typed-component mutation written to HEAD outside
  the normal snapshot/commit boundary, vs
- (b) the benign steady state: HEAD already holds the last finalized `### Re:`
  response and the current operation (compact archival, or a recovery commit) is
  legitimately re-deriving from it.

Compact's post-archival finalize tries to `auto-adopt` the post-compact content as
a patchback; the guard sees the `### Re: do [#rtwbcast]` marker already in HEAD,
classifies it as out-of-band, and fails closed — even though that marker is the
expected committed prior response.

## Fix direction (CLEAN session — touches the compact/commit drift guard)

1. Locate the guard emitting `refusing to auto-adopt committed historical response
   patchback … out-of-band typed_component_drift mutation` (likely in the
   compact/commit auto-adopt path; grep `typed_component_drift` /
   `auto-adopt committed historical`).
2. Make the discriminator structural, not marker-presence: a committed `### Re:`
   response in HEAD that MATCHES the snapshot's last-known committed response is the
   benign steady state and must NOT count as out-of-band drift. Only a HEAD typed
   component that diverges from the snapshot's committed baseline (content the
   current cycle never produced) is genuine `typed_component_drift`.
3. Specifically for compact: post-archival auto-adopt should compare against the
   pre-compact tagged state (`agent-doc/<doc>/pre-compact-<n>`) rather than treating
   any committed response marker as drift.
4. Add SimWorld / unit coverage: (i) compact of a doc whose HEAD holds a normal
   finalized `### Re:` response succeeds; (ii) a genuine out-of-band typed-component
   mutation still fails closed.
5. `make check` + full workspace 0 failed; live JB `Compact Exchange` re-verify is
   operator-drive.

## Resolution (2026-06-13, agent-doc source `fe4cf0d5`)

Investigated the guard end-to-end and confirmed the false-positive **does not
reproduce on current source** for the benign compaction class. The bail lives in
`agent-doc-orchestration/src/git.rs` `commit_with_outcome` (the
`refusing to auto-adopt committed historical response patchback` arm, ~line 761)
and fires only when ALL of:

1. `snapshot != file` (`!snapshot_matches_current_file`), AND
2. `committed_historical_patchback` yields a `kind` — either
   `classify_committed_historical_agent_doc_mutation(snapshot, head)` returns
   `Some`, OR `has_non_exchange_component_drift_scoped(snapshot, head, turn_scope)`
   is true (→ `typed_component_drift`), AND
3. `detect_bypassed_response_write_between(snapshot, head)` finds a `### Re:`
   heading present in HEAD but absent from the snapshot, AND
4. `repair_committed_historical_snapshot_drift` did not already self-heal it.

A **clean exchange-only compaction** (responses archived to a Session Summary,
every NON-exchange component preserved) never satisfies condition 2's
`typed_component_drift` branch — `has_non_exchange_component_drift_scoped`
skips exchange — so the bail is not reached. The `#nm1x` turn-scope intersection
(already in source) closed the remaining benign cases.

**Regression coverage landed** (`git.rs`):
- `commit_allows_clean_exchange_only_compaction` — HEAD holds a finalized
  `### Re: do [#rtwbcast]` response, post-compact snapshot+file archive it,
  status/backlog preserved → `commit` adopts the compacted doc (no bail).
- `commit_recovers_stale_pre_compact_snapshot_without_wedge` — the reported
  "snapshot stale at pre-compact size vs compacted visible file" recovery shape,
  no concurrent wedge → `commit` recovers without failing closed.
- Genuine out-of-band drift still fails closed: existing
  `commit_fails_closed_when_committed_historical_response_mutates_status` +
  `commit_blocks_committed_historical_patchback_that_mutates_status` (unchanged,
  still green). `make check` = 4405 passed / 0 failed.

**Residual:** the live repro's `typed_component_drift` bail required a CONCURRENT
non-exchange drift between snapshot and HEAD with `file != snapshot` — i.e. a
wedged `--handoff-state preparing` controller writing stale `content_ours` over
`ipc.sock`. That is the `#stuckhandoff2` wedge manifesting through this guard, and
blocking a wedged stale write is conservative-correct, not a guard false-positive.
The M1 self-watchdog suicide-timer for that wedge is now in source
(`project_controller::tests::self_watchdog_suicides_and_marks_failed_on_stale_preparing`
passes). Any live JB `Compact Exchange` failure that persists from a NON-wedged
session should be filed against `#stuckhandoff2`, not here. Live JB re-verify is
operator-drive.

## Notes

- The live binary at repro time predated source commit `ef0810a2` (preflight stale
  warning) — confirm the guard still false-positives on a freshly `make install`ed
  binary before deep changes; part of this may already be addressed in newer source.
- Related: `#jbcompactcrdt` (compact editor-IPC writeback), `#jb-compact-malformed-response-commit`
  (post-compact structural validation), `#ipcdrift`/`#stuckhandoff2` (the wedged
  controller that produces the drift the guard is trying to catch).
