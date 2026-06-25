# Plan: cycle_state advisory lock (lost-update window) — `#lzsidecarlock`

Sibling to `#lzsidecaratomic` (atomic sidecar writes — DONE, agent-doc `647af082`). This
plan closes the **lost-update** window among concurrent cycle_state writers.

## Problem

`cycle_state::save` is now atomic (temp-file + `persist`), so concurrent readers can no
longer see a torn file. But the **load→mutate→save** window is still unsynchronized: ~24
public mutators in `cycle_state.rs` (`start_preflight_with_task`, `mark_write_applied`,
`mark_response_captured`, `mark_committed`, `mark_abandoned`, all `record_*`,
`mark_pending_*`, `mark_recoverable_preflight_timeout`, `record_open_cycle_progress`, …)
each do `load(file)? → mutate one field → save(file, &state)?` with no lock or CAS.

Two writers running concurrently both `load` the same base; the last `save` wins and the
loser's mutation is clobbered. `cycle_phase_rank` (`cycle_state.rs:1273`) guards only
**monotonic forward phase** — it does NOT protect the ~30 non-phase fields
(`pending_done_ids`, `active_queue_heads`, `capture_id`, `file_hash`,
`ipc_snapshot_adoption_blocked`, `dropped_*_prompts`, …). A `mark_committed` racing a
`record_pending_done_ids` loses the pending-ids change.

Concurrent writers are rare in practice (closeout is largely sequential:
preflight → harness → finalize → session-check), but they exist: codex `Stop` hook vs
another writer; editor-IPC/CRDT convergence vs `git::commit`; the watch/idle-watch daemon
vs a CLI closeout. This is **hardening, not a live fire** — phase itself is safe.

## Deadlock analysis (DONE — safe)

A non-reentrant advisory lock is safe because **no production mutator calls another public
mutator**. Verified by grepping `cycle_state.rs`: the only nested mutator calls
(`start_preflight(…)` / `mark_*(…)` inside other mutators) are all in `#[cfg(test)]` code
(line ≥ ~1505). Production mutators each independently `load → mutate → save`. So wrapping
each in a non-reentrant `flock` cannot self-deadlock.

## Design

1. **Dedicated `.lock` sibling, not the state file.** The state file is atomically
   *replaced* (rename) on every save, so its inode changes; a flock on it would lock the
   old inode and a new writer would open a different inode → no mutual exclusion. Lock a
   stable sibling instead: `state_path.with_extension("json.lock")`
   (`.agent-doc/state/cycles/<hash>.json.lock`).

2. **RAII guard.**
   ```rust
   fn acquire_state_lock(file: &Path) -> Result<Option<StateLockGuard>> {
       let Some(state_path) = state_path(file)? else { return Ok(None); };
       let Some(parent) = state_path.parent() else { return Ok(None); };
       std::fs::create_dir_all(parent)?;
       let lock_path = state_path.with_extension("json.lock");
       let lock = std::fs::OpenOptions::new()
           .create(true).read(true).write(true)
           .open(&lock_path)?;
       // Bounded spin (fs2::FileExt::try_lock_exclusive): ~5s total. Avoids indefinite
       // block if a writer wedges while holding the lock. fcntl advisory locks release
       // automatically on process exit (even kill -9), so a dead holder frees immediately.
       let mut waited = Duration::ZERO;
       let step = Duration::from_millis(50);
       loop {
           if lock.try_lock_exclusive().is_ok() {
               return Ok(Some(StateLockGuard { _file: lock }));
           }
           if waited >= Duration::from_secs(5) {
               anyhow::bail!("cycle_state lock contention on {}", file.display());
           }
           std::thread::sleep(step);
           waited += step;
       }
   }

   pub(crate) struct StateLockGuard { _file: std::fs::File } // Drop releases the lock
   ```
   Needs `use fs2::FileExt;` and `use std::time::Duration;` (`fs2` is already a prod dep).

3. **One line per mutator.** Add `let _lock = acquire_state_lock(file)?;` as the first
   statement of each of the ~24 public mutators that do load→mutate→save. The guard drops
   at fn return (after post-save side effects like `append_phase_event_to_session_log` /
   `clear_pipeline_frontmatter`, which stay under the lock — fine). `start_preflight` only
   delegates to `start_preflight_with_task`, so lock only the impl.

4. **Return-`None`-on-no-state mutators** (`record_*` that return `Ok(None)` when
   `load(file)?` is `None`) still acquire the lock first so a concurrent creator can't race
   the `None` check.

## Test

Add `concurrent_writers_dont_lose_updates_under_lock`:
`start_preflight` → spawn two threads (`record_pending_done_ids(["a1","a2"])` and
`mark_committed`) → join → `load` and assert `phase == Committed` AND `pending_done_ids`
contains `a1`,`a2`. Serialized under the lock, both mutations always land (the second
writer loads the first's saved state). Without the lock this test can flake (lost update).

## Verification

- `cargo fmt --check`, `cargo clippy -p agent-doc-orchestration --lib` (cycle_state.rs must
  stay clippy-clean; note the orchestration crate has ~40 PRE-EXISTING clippy errors in
  `sync.rs`/`write/ipc.rs` on Rust 1.96 — unrelated, do not block).
- `cargo test -p agent-doc-orchestration --lib cycle_state` (existing 39 + new concurrency
  test).
- `make check`.
- Commit (agent-doc submodule) + push + superproject gitlink bump.

## Scope note

`#lzsidecarlock` is closeout-critical code (every cycle touches it). The edit surface
(~24 mutators) and the new failure mode (lock-contention timeout) justify a focused session
with a dedicated test/review pass rather than a tail-of-session rush. The atomic-write
half (`#lzsidecaratomic`) — the part that prevented the live torn-read class — is already
shipped and installed.
