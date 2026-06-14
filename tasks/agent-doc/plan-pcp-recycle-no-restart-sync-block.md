# Plan — PCP/Supervisor recycle does not restart the agent session; JB Sync Tmux Layout blocks on an in-flight sync guard

Bug id: `#recyclerestart`
Tag: `[clean-session]` (live write path + tmux pane management; must not run under the live route-owned supervisor)

## Progress (2026-06-14)

- **Q1 (recycle ENOENT / recycle does not restart the session) — CODE-COMPLETE.**
  `supervisor_reexec_candidates` / `build_reexec_candidates` (start.rs) already drop a
  `(deleted)` `current_exe`, prefer the canonical freshly-installed binary
  (`current_agent_doc_binary()`), and end with a bare-`agent-doc` PATH fallback, with
  per-candidate `exists_before`/errno logging. The keep-pane-alive `reexec_failed`
  fallback matches `#aazp`/`#4myd`. Live recycle proof remains operator-verify.
- **Q2 (sync guard permanently held) — FFI self-heal LANDED (this session).** The
  cross-editor guard (`ffi.rs` `agent_doc_sync_try_lock`) was a bare `AtomicBool`, so a
  sync thread that wedged on in-pane recovery and never called `agent_doc_sync_unlock`
  held it forever → every later `Sync Tmux Layout` deferred with "another sync is already
  running". Now the guard tracks the holder's acquisition time
  (`SYNC_LOCK_ACQUIRED_AT_MS`) and a later acquire **supersedes** a holder wedged past
  `DEFAULT_SYNC_LOCK_STALE_BOUND_MS` (45s, above the plugin's 30s subprocess timeout +
  margin) — emitting `sync_guard_released reason=stale_holder_superseded held_ms=…`. A
  legitimately in-flight sync (held under the bound) still defers, and the CLI's own
  `.agent-doc/sync.lock` file-lock (with stale-orphan reaping) prevents a double-run. The
  JB plugin already calls `agent_doc_sync_try_lock()`, so the self-heal takes effect on a
  cdylib refresh (`cargo build --release && agent-doc lib-install`) with NO Kotlin change.
  Pure decision (`sync_lock_acquire_decision`) + the FFI path are unit-tested.
- **Q2 remaining (deeper, separate rung):** decouple the *geometry* reconcile from
  in-pane init/recovery in `agent-doc sync` (CLI phase-split: hold the guard only across
  the fast geometry phase, run pane readiness/recovery guard-free), and make a newer sync
  *generation* supersede an in-flight stale one (the plugin already has
  `agent_doc_sync_bump_generation` / `check_generation`). Add JB `SyncLayoutActionTest`
  coverage + a SimWorld kill-pane-then-sync case. This is a focused clean-session JB+CLI
  rung; the FFI self-heal above already removes the permanently-wedged-guard failure the
  operator hit.

## Operator-stated symptom (2026-06-13)

1. **Recycle does not restart the agent session.** The PCP/Supervisor "recycle"
   (idle-boundary binary promotion / `/clear`-and-continue) is expected to hand
   the next queue item to a freshly-cleared agent session, but the operator
   observed the agent session was **not** restarted by the recycle. Screenshot
   `img_21.png`.
2. **JB `Sync Tmux Layout` deferred.** After `tmux kill-pane`, clicking JB
   `Sync Tmux Layout` surfaced:
   `Sync deferred: another tmux layout sync is already running; the latest editor selection will retry shortly.`
   Operator framing: "Why was the sync blocked? The syncs should be fast. Are
   they blocking on something? If there is some in-pane initialization needed,
   the tmux pane management should be independent."

## Code anchors

- JB deferred message #2:
  `editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/SyncLayoutAction.kt:33`
  (the plugin-local sync guard). Message #1 (`:31`) is the mid-closeout defer.
- Intended contract already in SPEC:
  `editors/jetbrains/SPEC.md:49` — "JetBrains must bound every plugin-spawned
  sync subprocess. On timeout, it terminates the process, releases the
  plugin-local sync guard, logs the timeout … must not leave the IDE
  permanently reporting `Sync deferred: another tmux layout sync is already
  running`."
- Recycle / idle-boundary promotion: `agent-doc-orchestration/src/start/idle_watch.rs`,
  `agent-doc-orchestration/src/start/run.rs` (recycle), `project_controller.rs`
  (session restart). Note: these files carry **uncommitted** work in the
  current dirty submodule tree — re-baseline before touching.

## Confirmed root cause (2026-06-13, read-only diagnosis — screenshots img_21/img_22)

Both screenshots show the recycle path logging
`supervisor execve hot-reload failed (No such file or directory (os error 2)); falling back to clean exit`,
then the pane either dies (`Pane is dead (status 0)`) or leaves the harness
child hung (`Cultivating... still thinking`). The recycle is NOT a session
restart — it is a supervisor self-terminate that nothing relaunches.

1. **Recycle ENOENT (Q1).** `supervisor_perform_reexec`
   (`agent-doc-orchestration/src/start.rs:1717`) re-execs
   `std::env::current_exe()`. `make install` / `cargo install` replaces the
   binary inode atomically (build temp → `rename()` over the target), unlinking
   the inode the running supervisor holds. On Linux `current_exe()` reads
   `/proc/self/exe`, which now resolves to the deleted inode and returns a path
   with a trailing ` (deleted)` suffix → `Command::new(exe).exec()` fails ENOENT
   (os error 2). The `Err` arm logs and `std::process::exit(0)`. The
   "next launch uses the new build" comment assumes something relaunches the
   supervisor; in a tmux-hosted pane nothing does, so the agent session is left
   dead or wedged. **Fix (clean session):** resolve the re-exec target to the
   canonical freshly-installed binary path instead of `current_exe()` — strip a
   trailing ` (deleted)` and re-canonicalize / re-stat, or use the install path
   that the staleness detector already discovered (`preflight.rs:898` knows the
   `~/.cargo/bin` target). The fallback arm must genuinely restart/clear the
   harness child (cold relaunch in the same pane), not bare `exit(0)`.

2. **Sync defer is downstream of Q1 (Q2).** The JB plugin's
   `agent_doc_sync_try_lock()` guards on an **in-process** `AtomicBool
   SYNC_LOCKED` in the loaded cdylib (`src/ffi.rs:78`). `SyncLayoutAction`
   spawns `agent-doc sync` on a worker thread holding that guard and releases it
   in `finally` (`SyncLayoutAction.kt:331`); the subprocess is bounded to 30s
   (`SYNC_PROCESS_TIMEOUT_MS`, `:34`). When the pane from Q1 is wedged, an
   in-flight `agent-doc sync` blocks on it, so `SYNC_LOCKED` stays held and the
   next manual click defers with `SYNC_ALREADY_RUNNING_WARNING` (`:33`). This is
   the operator's "tmux pane management should be independent of in-pane init":
   the layout-geometry sync should not block behind a sync that is stuck on the
   supervisor pane. Note the CLI `acquire_sync_lock` path already reaps stale
   orphaned `.agent-doc/sync.lock` owners (`sync.rs` `is_stale_orphaned_sync_lock_owner`),
   but the plugin's in-process `try_lock` fails fast on contention without that
   recovery and without decoupling geometry sync from in-pane readiness.

## Hypotheses to verify (clean session)

- **Sync guard held by a slow/blocking sync subprocess.** The plugin-local
  guard is acquired before spawning `agent-doc sync` and is meant to be bounded
  (SPEC `:49`). If a prior sync subprocess stalled (e.g. on a dead/recycling
  pane or in-pane initialization), the timeout/terminate/release path is either
  not firing or its bound is too long, so the next click sees the guard held →
  the deferred dialog. Verify the bound exists and actually releases on a
  killed/recycling pane.
- **Pane management coupled to in-pane init.** Operator wants tmux pane
  open/move/sync independent of in-pane agent initialization. If `agent-doc
  sync` blocks on probing/initializing the in-pane process, decouple the
  layout-geometry sync from in-pane readiness probing.
- **Recycle not restarting the session.** Confirm whether the idle-boundary
  recycle is expected to restart the harness session vs only promote the binary;
  reconcile against the operator's mental model (recycle == fresh-cleared agent
  for the next queue item). Cross-ref `plan-proactive-recycle-on-install.md`
  and `plan-s760-transcript-ctx-clear.md`.

## Acceptance

- Repeated `Sync Tmux Layout` after a `tmux kill-pane` / recycle never leaves a
  permanently-held guard; a stalled sync is bounded, terminated, and the guard
  released within the SPEC bound, with a `sync_guard_released` proof line.
- Layout geometry sync does not block on in-pane initialization.
- Recycle restarts (or provably re-clears) the agent session for the next queue
  item, with an ops.log proof line distinguishing "binary promoted" from
  "session restarted/cleared".
- SimWorld + JB `SyncLayoutActionTest` coverage for the stalled-guard release
  and the kill-pane-then-sync path.

DO NOT IMPLEMENT under the live supervisor — clean session required.
