# Plan — finalize stall on the IntelliJ-vs-agent visible-buffer race (IPC-timeout fallback)

**Backlog:** `#finalize-visible-buffer-ipc-timeout-race` in `tasks/agent-doc/agent-doc-bugs2.md`
**Status:** Draft — needs live-IntelliJ reproduction before any guard change (fail-closed, data-loss-sensitive).
**Reported:** 2026-06-05 — an agent-doc turn on `equityfundingsource.md` stalled at finalize with: *"My response can't commit because IntelliJ and the agent are racing on the doc, and I'm independently blocked on the images. I'm halting rather than fighting the buffer."*

## Symptom

`agent-doc finalize --stream` does not commit; the turn halts with a "racing on the
doc / fighting the buffer" message while IntelliJ has the document open. The
response is lost for that cycle (must be re-driven). Memory records this as a
recurring class: "Queue auto-loop + live IntelliJ = finalize-guard recovery
treadmill" and "Visible-write guard + git-checkout file_hash wedge".

## Mechanism (confirmed in code)

Path: `write.rs` `run_template`/`run_stream` → IPC-first write. When `.agent-doc/patches/`
exists (plugin installed) the write is sent over IPC for the IDE to apply to its
live buffer. If the IDE does not ACK within the 2s IPC window, the code enters the
**IPC-timeout fallback** (`write.rs` ~7595-7683):

1. `guard_visible_write_idle_and_current(file, "run_stream_ipc_timeout", &content_current)`
   (write.rs:7629) runs before the local disk write.
2. That guard (`guard_visible_write_idle_and_current`, write.rs:5838) does:
   - `guard_visible_write_idle` — waits for the typing indicator to settle.
   - `live_buffer_diverges_from_content(indicator_path, expected_current)` — compares
     the IDE-reported live-buffer digest sidecar against the expected disk state.
   - re-reads the file and compares to `expected_current`.
3. If the live buffer digest diverges, the guard `bail!`s ("visible editor buffer …
   differs from the expected disk state"). In the fallback the caller logs
   `visible_write_deferred_live_buffer_changed`, prints the WARNING, and
   `std::process::exit(75)` (EX_TEMPFAIL) **without writing or committing**
   (write.rs:7633-7639). That non-persisting exit is the stall.

So the race window is: **IPC times out (IDE busy/slow to ACK) AND the live-buffer
digest no longer matches expected** → fail-closed, no persistence.

## Why it is fail-closed by design

The guard exists to prevent the agent's disk write from clobbering genuine unsaved
IDE edits (the IDE would otherwise overwrite the disk write on its next flush, or
the disk write would drop the user's in-buffer edits). Loosening it incorrectly is a
**data-loss risk**. Any fix must preserve that protection for the genuine
"user has real unsaved edits" case.

## Hypotheses to test (need live repro)

1. **Stale-digest false positive.** When the agent's own IPC patch *was* applied by
   the IDE just after the 2s timeout, the live-buffer digest may now equal the
   agent's intended content while `expected_current` is the pre-write `content_current`
   — so the divergence is against the wrong baseline and the guard fires spuriously.
   Test: capture `expected_hash`/`expected_len` vs `live_hash`/`live_len` from
   `ops.log` at the moment of the stall and compare to the post-write content hash.
2. **ACK latency, not a real edit.** The IDE is merely slow (GC/indexing) and the
   buffer is not actually divergent; a longer/adaptive IPC window or a post-timeout
   re-poll of the ACK would let the IPC path complete instead of falling through.
3. **Genuine divergence.** The user really did edit the buffer; the correct behavior
   is to merge (CRDT) or surface a precise, recoverable prompt — not lose the response.

## Candidate fixes (decide after repro)

- **A. Re-poll ACK after timeout before falling through.** Before the disk fallback,
  re-check whether the IDE applied the patch (sentinel / digest now equals intended
  content); if so, treat as IPC success and commit via the IPC snapshot path.
- **B. Compare against the right baseline.** Have the timeout fallback compare the
  live digest to the agent's *intended* `content_ours`/`final_content`, not the
  pre-write `content_current`, so a successfully-applied-but-late IPC patch is not
  read as divergence.
- **C. CRDT-merge instead of bail.** When divergence is real, run the existing
  3-way/CRDT merge (`merge.rs`) of (snapshot baseline, agent response, live buffer)
  and persist the merged result via IPC, rather than exit(75).
- **D. Persist-then-defer.** Never exit(75) with the response unpersisted: always
  durably capture the response (capture ledger already exists) so the next preflight
  can replay it deterministically, even when the visible write is deferred.

Option D (or B) is likely the lowest-risk first step: it removes the *data-loss* edge
(lost response) without loosening the clobber guard.

## Exit criteria

- A live IntelliJ session reproduces the stall with `ops.log` evidence
  (`visible_write_deferred_live_buffer_changed` with the hash/len triple).
- The chosen fix lets finalize either complete (IPC/merge) or defer **without losing
  the response** (replayable on the next cycle), while still refusing to clobber
  genuine unsaved IDE edits.
- Deterministic SimWorld coverage for the IPC-timeout + divergent-digest race
  (extend the existing IPC listener tests in `preflight.rs`/`write.rs`).
- The "images blocked" half of the original report is tracked separately — it is an
  unrelated content blocker, not part of the buffer race.

## Why deferred (not blind-fixed in the auto-loop)

The guard is fail-closed and data-loss-sensitive, the fix depends on which hypothesis
the live `ops.log` confirms, and validation requires live IntelliJ. An unattended
guess risks either clobbering user edits (loosening the guard) or masking the real
cause. Resolve in a reviewed session with reproduction in hand.

## Live repro captured 2026-06-06 (agent-doc-bugs2.md, during #autoloop-command-preemption cycle)

Clean, high-signal reproduction while draining the `go` queue:

- `agent-doc finalize --stream` → `socket IPC ack timeout (2s)` → file-IPC fallback →
  `IPC timeout (2s)` → `visible write deferred before exit(75)`. Retried with
  `--force-disk`; the same guard still blocked the write (force-disk respects the
  visible-buffer guard — useful data point: `--force-disk` is NOT an escape hatch here).
- `ops.log` `visible_write_deferred_live_buffer_changed`:
  `expected_len=57970 expected_hash=b175f050…` vs **`live_len=107235 live_hash=eba879ba…`**
  (`live_ts=1780726116023`). The live IntelliJ buffer is ~**2× the disk size** —
  duplicated content in the editor Document, not on disk.
- **Disk was provably clean the whole time:** `57970` bytes, byte-identical to committed
  `HEAD` (`d700d86a8`), 5 `### Re:` blocks, exactly one `agent:exchange` marker pair.
  So the divergence is purely editor-side buffer doubling; disk/HEAD never corrupted.

Implication for the fix hypotheses: the failure here is the editor Document ballooning to
2× (duplicate-apply in the plugin), with the guard correctly refusing. A viable recovery
that needs validating: when the live buffer diverges AND disk == HEAD (provably clean),
offer a "reload editor buffer from the clean disk/HEAD, then re-apply the pending response"
path instead of a bare exit(75) that strands the response. Operator recovery that worked
to unblock: revert/reload the IntelliJ buffer from disk, then re-run finalize.

## Root-cause investigation 2026-06-06 + shipped partial fix

Traced the IPC write path (Rust `write.rs` + Kotlin `PatchWatcher.kt` + core
`append_with_boundary`). Findings:

1. **Divergent boundaries per rebuild (FIXED).** One finalize builds its IPC patches up
   to three times — socket attempt (`try_ipc` ~10244), file-IPC fallback (`try_ipc` ~10670),
   and `run_stream` timeout re-write (~7503). Each call ran
   `reposition_boundary_to_end_clean_with_summary`, which minted a FRESH RANDOM boundary,
   so the same response was sent under different boundary IDs (live repro logged
   `60aecabf` then `fc36ee6c`). When the socket patch lands but its ACK times out, the
   binary re-sends under a new boundary the plugin can't match — `append_with_boundary`
   then falls through (boundary not found) to an unconditional `append_with_caret`, re-adding
   content. **Fix shipped:** derive the boundary deterministically from the stable `patch_id`
   (`id::boundary_id_from_seed_with_summary`), threaded through `build_ipc_patches_json` to
   all rebuild sites, so every rebuild of one write carries an IDENTICAL boundary. Unit-tested
   (`boundary_id_from_seed_*`, `build_ipc_patches_json_seeded_boundary_is_stable_across_rebuilds`).

2. **Content dedup is robust to (HEAD)/boundary (NOT the miss).** `normalize_append_patch_content`
   already strips ` (HEAD)` and boundary markers before the `contains` check, so the
   post-apply `(HEAD)` annotation does not by itself defeat `append_patch_already_present`.

3. **Residual — full-doc-scale duplication (STILL GATED on live-verify).** The live buffer
   reached `107235` ≈ **85% of the `57970` doc duplicated** — far larger than just the
   exchange response. That near-full-document signature points beyond the boundary-divergence
   to a full-document merge/apply path — likely the `#ipcfullprompt` class the plugin
   CLAUDE.md documents ("two divergent baselines merge-applied twice" via duplicate
   PromptPoller tracking keys). The deterministic-boundary fix removes one divergence vector
   and makes rebuilds idempotent, but whether it eliminates the ~2× duplication CANNOT be
   confirmed without a live-IntelliJ repro. Remaining hardening to verify live:
   - Plugin-side atomic `patch_id` dedup: `recordApplied` BEFORE the async apply (or
     check-and-set) so two patches sharing one `patch_id` can't both apply during the race.
   - Confirm PromptPoller tracks the doc under exactly one canonical key during a
     finalize-timeout (guard against the `#ipcfullprompt` merge-applied-twice path).
   - The exit(75) recovery: reload-from-clean-disk-then-reapply instead of stranding.
