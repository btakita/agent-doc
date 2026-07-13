# Plan: preflight → lazily state-diff feed

Operator directive (2026-07-13): *"agent-doc should not have failed preflight. The
realtime document model should handle the typing. Replace preflight to have a
lazily state diff feed to prompt the agent with the diff and realtime steering."*

## What preflight actually is (so the "replace" is scoped, not a rewrite)

`agent-doc preflight <FILE>` is the pre-agent cycle-contract command. Steps:

0. **layout check** — tmux/editor pane sanity.
1. **repair** — interrupted-cycle recovery + idempotent pending maintenance
   (mirror reap, dedupe, backfill, status-marker reconcile).
2. **commit** — commit the prior response snapshot if uncommitted.
3. **claims / related docs** — cross-doc ownership + related-change surface.
4. **diff** — `agent_doc_diff_io::compute_with_current`: diff the committed
   **snapshot** (`previous`) against the **current** document, save the merge
   `baseline_file`, and emit the JSON cycle contract (`diff`, `diff_type`,
   tiers, `queue_*`, `prompt_targets`, realtime-steering set, …).

The agent-facing product of preflight is **step 4**: the *state diff* (what the
operator changed since the last committed turn) plus the realtime-steering
aggregate. Steps 0–3 are housekeeping. "Replace preflight with a lazily state
diff feed" = **re-source step 4's `current` from the lazily reactive CRDT model**
and stop letting housekeeping (steps 1–2) fail-closed on realtime buffer drift.

## Why it failed even though the operator was not typing (FIXED)

Step 1 reaped an already-done mirror queue item, then tried a *visible write* to
persist the reap. That write went through the **fail-closed** guard
(`guard_visible_write_idle_and_current`). The live buffer had drifted from
preflight's `expected_current` — not from typing, but because the last committed
response was still being reconciled back into the live editor buffer by the
realtime replica. The guard returned `Err`, and preflight aborted the whole
cycle.

**Fix landed (`b2ba6735`, `#realtime-maintenance-defer`):** idempotent pending
maintenance now *defers* to a later cycle on realtime buffer drift
(`document changed after the response merge was computed` / `editor typing did
not settle`) instead of aborting. Narrower than `deferable_status_error`: an
*unresolvable* authority still fails closed so a closeout never writes behind an
active listener. Regression test:
`pending_maintenance_defers_mirror_reap_when_realtime_buffer_drifted`.

## Current diff source (the part still to migrate)

`compute_with_current` → `wait_for_stable_content(doc, previous)`:
- **editor-authoritative path**: wait for the editor plugin to report a stable
  buffer (debounce 500ms, timeout 6s), then **read the disk file** as `current`,
  with a hash-mismatch fallback to disk.
- **fallback path**: truncation-heuristic disk re-reads.

So `current` is sourced from **disk + debounce-poll**, and the lazily CRDT relay
is consulted only as a *guard authority*, never as the diff's content source.
That is the disk-race surface the operator wants retired.

## Target: lazily state-diff feed

Source `current` for the diff directly from the lazily reactive CRDT canonical
text (`agent_doc_crdt_relay_io::CurrentText::Current { text, .. }`) when the
relay reports the model current, instead of racing disk reads. The CRDT model is
always internally coherent under concurrent typing, so the feed is:

    diff( committed_baseline , lazily_canonical_current )  +  RealtimeSteeringSet

Keep a *prompt-completeness* settle (do not diff a half-typed prompt line) but
drive it off the reactive model's quiescence signal, not a disk mtime poll. Fall
back to the existing disk+debounce path only when the relay is detached /
unavailable (`Detached`, `EditorAttachedMissingReplica`, `EditorSyncPending`).

### Phases

1. **`current` from relay (spike + guard). — DONE (`948c74b5`).** Added a
   `LiveCurrentSource` DI seam to `agent-doc-diff-io` (it is a leaf beneath
   `crdt-relay-io → snapshot-io → diff-io`, so a direct dep would cycle).
   `wait_for_stable_content` settles on disk first (prompt-completeness), then
   sources `current` from the reactive model via `durable_buffer_state`, which
   returns `Some` only when a live editor buffer diverged from disk and `None`
   otherwise (byte-identical to the disk path at rest). Wired at the single
   `compute_with_current` production caller in `preflight-command-io`. Tests:
   `wait_for_stable_content_prefers_live_reactive_over_disk`,
   `wait_for_stable_content_none_live_matches_disk_byte_for_byte`,
   `compute_with_current_uses_live_reactive_content_for_diff`.
2. **Baseline alignment. — DONE.** The response baseline is saved from
   `diff_result_with_current.current` (`run.rs`), which is now the
   reactive-sourced `current`, so the finalize merge baseline matches the buffer
   the operator sees. The `#qconvbaseline` realign is a single guarded step that
   is a no-op when queue maintenance converged nothing; reactive sourcing makes
   the baseline already match the buffer, shrinking (not duplicating) its work.
   No double-realign — there is exactly one `realign_baseline_to_converged_queue`
   call.
3. **Quiescence settle. — DONE.** The reactive read is its OWN quiescence gate:
   `crdt-relay-io` surfaces `CurrentText::Current` only once the commit barrier
   holds — the canonical replica *covers every live editor's op*
   (`commit_barrier_ready` in `agent-doc-merge/src/crdt_sync.rs`) — so the
   returned text is prompt-complete by construction, not a mid-typing partial.
   `delivery_converged` (a downstream fan-out ACK signal) is deliberately NOT
   gated on: it is a different direction and would reject complete canonical text
   over a pending ACK. `wait_for_stable_content` is now **reactive-first** — it
   reads disk once (no debounce) only to detect divergence, then returns the
   commit-barrier-gated reactive text, skipping the disk-settle debounce entirely
   (test `wait_for_stable_content_reactive_first_skips_dirty_disk_debounce`).
4. **Retire disk race. — DONE.** Reactive-sourced `current` is the default in
   preflight (unconditional live source), and the disk-settle debounce is demoted
   to the fallback taken only when the reactive read yields nothing (no
   divergence: reactive == disk; or the model is unavailable/detached; or no live
   source). At rest the feed is byte-identical to the old disk path
   (`wait_for_stable_content_none_live_matches_disk_byte_for_byte`).

### Risks / guardrails

- A mid-typing relay snapshot must not feed a **half-typed prompt** to the agent
  — keep a settle gate (phase 3), just drive it off the reactive model.
- `current` feeds baseline save + `#qconvbaseline` + snapshot recovery; a subtly
  different `current` ripples. Phase 1 must prove byte-identity at rest before
  phase 4 flips the default.
- This is the S6 leg of the live-editor reactive-family / sidecar-retirement
  initiative — align with `plan-live-editor-reactive-backbone.md` and
  `plan-sidecar-retirement-lazily-sync.md`; do not fork a second merge path.
