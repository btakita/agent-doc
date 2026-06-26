# Plan: Mid-turn recycle/restart leaves an open cycle → `live_prompt_drift_after_preflight` wedge (`#midturn-recycle-resume`)

## Operator directive

> "We should have been able to recycle or restart the supervisor mid turn. The restart should reliably restart the turn."
> "Fix the root cause of the wedge...which is likely a mid-turn restart/recycle supervisor."

## Observed wedge chain (live session b26b9957)

1. `finalize` → `ipc_proof_insufficient ... invariant=live_prompt_drift_after_preflight recovery=content_ours_snapshot_next_cycle`.
2. Recovery appends a binary-authored `### Re: IPC proof diagnostic` block and commits via `content_ours`.
3. Next `finalize` REFUSED: "previous cycle is still `preflight_started`, unresolved prompt_target: ipc_proof_insufficient ... no response exists to replay".
4. Fallback `finalize --force-disk` → `postcommit_worktree_check match=false` corruption + `direct_response_patchback` INTERRUPT.
5. Recovery needs `git checkout HEAD -- <doc>` + `reset --from-current --preserve-session`.

This happened **right after a mid-session supervisor recycle** (route-owned parent re-exec'd onto a freshly-installed binary).

## Root cause (CONFIRMED — operator hypothesis holds, with a precise mechanism)

### The recycle is a true `execve` process-image replacement

`supervisor_perform_reexec` (`start.rs:1859-1924`) calls `std::os::unix::process::CommandExt::exec()` (`start.rs:1908`) — `execvp(2)`, which replaces the process image. **All in-memory supervisor state is discarded**; only the live harness child PTY survives (master fd `dup`+CLOEXEC-cleared and marshaled through env, `start.rs:1871-1892`). The fresh process re-enters and *adopts* the child without re-triggering it (`start/run.rs:1036-1052`, "the adopted child is mid-session and must not be re-triggered").

### The fresh supervisor holds NO in-memory CRDT — it reloads from disk + journal

`start/run.rs` constructs no CRDT replica / live-buffer at startup. On boot it reads the doc from disk (`run.rs:195`), folds any unflushed `.agent-doc/live-buffer/<hash>` operator edit into the queue journal (`run.rs:217` `record_live_buffer`), and replays journaled queue prompts missing from disk (`run.rs:223-224`). The CRDT lives on disk (`.agent-doc/crdt/<hash>.yrs`) and is rebuilt **per write-cycle** by the write/merge path, not held as long-lived supervisor state. So a recycle does **not** leave a *stale in-memory CRDT replica* (the operator's literal wording) — there is no such replica. The drift comes from a different, sharper place: **the in-flight IPC connection of the current cycle is severed by the `execve`.**

### The actual gap: the recycle gate guards the HARNESS turn, NOT the agent-doc cycle

The recycle decision (`start/idle_watch.rs:1461-1681`, via `supervisor_recycle_action` in `start/decisions.rs:505-553`) gates only on:

```rust
let turn_boundary = prompt_visible && !turn_active;   // idle_watch.rs:1155
if !turn_boundary { return SupervisorRecycleAction::None; }  // decisions.rs:515
```

`turn_active` is the **harness whole-turn** marker (`turn_status.rs`: set by `UserPromptSubmit`, cleared by `Stop` — spans the entire agent turn, TTL 3600s). The agent-doc `preflight → finalize` cycle runs as sub-steps *inside* one harness turn. So `turn_boundary` correctly defers across a normal in-turn cycle **as long as the harness marker is fresh and accurate**.

But `do_recycle` (`idle_watch.rs:1676-1682`) consults **ONLY** `recycle_action` (+ the idle-grace debounce). It does **NOT** read `cycle_state.phase` / `cycle_state.is_open()` or `ipc_socket::inflight_connection_handlers()`. Those facts feed a *different* decision — the convergence/drain gate (`idle_watch.rs:60-74`, `gather_convergence_facts`) — which controls inter-item *dispatch*, never the recycle.

**Consequence:** when the harness `turn_active` marker is missing / stale / expired, or there is any micro-window where the harness reports a boundary while an agent-doc cycle is still `preflight_started` / `response_captured` / `write_applied` (open, finalize not committed) with an in-flight IPC ack connection, `supervisor_recycle_action` returns `RecycleImmediate`/`RecycleDebounced` and the `execve` fires **mid-cycle**.

The `#supautoinstall` rung (`idle_watch.rs:1197-1258`) makes this the *common* case: after a finalize commits a source edit, the idle supervisor builds+installs the new binary IN the same idle-watch loop, then the recycle block immediately below sees the now-newer binary (`process_binary_is_stale`, `idle_watch.rs:1336`) and hot-reloads — exactly the "right after a mid-session recycle onto a freshly-installed binary" the operator observed.

### Why the severed cycle produces `live_prompt_drift_after_preflight`

At finalize, `guard_ipc_snapshot_adoption_against_live_prompt_drift` (`write/ipc.rs:696-789`) calls `ipc_snapshot_would_absorb_live_prompt_drift_after_preflight(baseline, snapshot_candidate, content_ours)` (`write.rs:528-550`):
- `baseline` = the preflight snapshot (taken **before** the recycle).
- `snapshot_candidate` = the IDE plugin's ack-content sidecar (`.agent-doc/ack-content/<patch_id>.md`, `write/ipc.rs:8-22`) — or, when no sidecar round-trips, a **disk-read fallback** (`write/ipc/transport.rs:1398-1405`, `ipc.rs:1223-1231`).
- `content_ours` = baseline + the agent response, no user edits.

When the `execve` severs the in-flight IPC listener, the ack-content sidecar never round-trips; the candidate falls back to whatever the recycled supervisor / disk now holds, which diverges from the pre-recycle `baseline` → the predicate flags a prompt-bearing change the response didn't author → drift raised → `content_ours_snapshot_next_cycle` (`ipc.rs:774-788`). The `content_ours` snapshot is now larger than the fragmented visible disk file, so the next commit's stale-snapshot guard fails closed (`write/converge.rs:350-357`), and `repair.rs:624-632` refuses the next finalize ("no response exists to replay"). Auto-recovery (`write/converge.rs:436-545`) fails its containment check when the disk file carries a prompt the snapshot lacks → the wedge persists.

### `#durablerecycle` checkpoint: written but NOT consumed to resume the turn

`record_turn_checkpoint` (`cycle_state.rs:486-535`) persists `baseline_file` / `prompt_targets` / `queue_task_id` / `turn_id` at preflight (`preflight/run.rs:760`). The only readers are:
- `closeout.rs:626-664` `open_cycle_recovery_command` — builds a recovery **message string** only.
- `idle_watch.rs:108` — `turn_id` for a convergence-playback artifact.
- `doctor.rs:399-402` — diagnostics passthrough.

**No reader re-dispatches or resumes the turn from the checkpoint.** The "re-dispatches the pending head on the fresh binary" comment (`idle_watch.rs:1151`) is realized purely by the surviving child continuing its own execution — the supervisor does not consume the checkpoint. So "the restart should reliably restart the turn" is only true incidentally (child survives); if the cycle was severed mid-finalize, the checkpoint does nothing to recover it.

## Hypothesis verdict

**CONFIRMED with refinement.** The operator's "stale in-memory CRDT" framing is mechanically inexact (there is no long-lived in-memory CRDT replica; the fresh process reloads from disk). The real defect is that the **recycle gate has no agent-doc-cycle / IPC-inflight interlock** — it only respects the coarse harness turn marker. A recycle can `execve` while an agent-doc cycle is open with an in-flight IPC ack connection, severing it and producing `live_prompt_drift_after_preflight` against the pre-recycle preflight baseline. The downstream `content_ours` wedge + refusal chain follows deterministically.

## Fix design (tightest, makes the drift impossible)

**Add a cycle-open / IPC-inflight interlock to the recycle gate** so an `execve` recycle can only fire at a TRUE quiescent boundary: harness turn boundary AND no open agent-doc cycle AND no in-flight IPC handler. This makes the mid-cycle severing impossible rather than recovering from it better.

### Phase A (this cycle — the root fix)

1. `supervisor_recycle_action` gains a `cycle_open: bool` parameter. When `cycle_open` is true, every recycle arm (RecycleImmediate / RecycleDebounced / EscalateKillRelaunch / the `#wd40` explicit-admin-on-fresh-binary flush) is deferred — return a new `SupervisorRecycleAction::DeferCycleOpen` (a no-op for this tick that re-evaluates next loop, exactly like `!turn_boundary` already defers). The deferral preserves the `#durablerecycle` checkpoint and the in-flight cycle; the recycle fires on the next loop once the cycle commits and IPC drains.
   - Rationale for deferring even `explicit_admin` / `write_wedged`: those still must not `execve` *mid-finalize* — they wait one cycle boundary (sub-second once finalize commits), which is strictly safer and still "immediate" from the operator's perspective. This mirrors the existing `AwaitDrain` contract for operator restart.

2. `idle_watch.rs` computes `cycle_open` from live runtime state before the recycle decision:
   ```rust
   let cycle_open = crate::cycle_state::load(&path)
       .ok().flatten().map(|s| s.is_open()).unwrap_or(false)
       || crate::ipc_socket::inflight_connection_handlers() > 0;
   ```
   and passes it to `supervisor_recycle_action`. The same `cycle_open` also gates the `#supkill-bg` `restart_drain_reexec` operator path and the `#supautoinstall` rung's immediately-following recycle (the auto-install build itself is fine; only the `execve` must wait for the cycle to close).

3. Emit a `supervisor_recycle_deferred_cycle_open` ops-log line so the deferral is diagnosable (mirrors the existing `supervisor_binary_stale_detected` provenance).

### Phase B (IMPLEMENTED — 0.34.50)

Actively CONSUME the `#durablerecycle` checkpoint on a fresh supervisor boot to re-dispatch a turn that was genuinely interrupted (child died across the recycle), instead of relying solely on the surviving child. This is the "restart should *reliably* restart the turn" half. Phase A removed the wedge by making the mid-cycle severing impossible; Phase B makes the residual genuinely-interrupted case (the child does NOT survive the recycle) reliably re-dispatch.

**Mechanism (the FIRST checkpoint consumer that resumes):**

1. **Boot-resume path** (`start/run.rs`, right after `ReexecState::from_env()`). A fresh supervisor image born from an `execve` recycle (`pending_adopt.is_some()`) loads the `#durablerecycle` checkpoint and routes through the pure `boot_resume_action(is_recycle_boot, cycle_open, child_survived, already_consumed)`:
   - **`RedispatchInterruptedTurn`** — cycle open AND child died AND not yet consumed: drop the dead-pid adopt (`pending_adopt = None`), set `auto_trigger_next_launch = true` so the first iteration spawns a fresh child and re-submits `agent-doc <FILE>` (re-running preflight against the still-open checkpoint, which re-drains the same `queue_task_id` / `prompt_targets` head), and latch `recycle_resume_consumed`.
   - **`AdoptSurvivingChild`** — cycle open AND child survived (the common case): adopt the live child WITHOUT re-triggering. The surviving child is still running the turn; re-dispatching would double-run it. This is the idempotency guard.
   - **`None`** — not a recycle boot, or a closed (committed/abandoned) / already-consumed checkpoint: resume nothing.

2. **"Child survived?" determination** (`ReexecState::child_survived()`, `start/decisions.rs`). The recycle marshals the harness child PID across the `execve` (env handoff). On boot the fresh image probes it with `kill(pid, 0)`: `Ok` (or `EPERM`) ⇒ the PID still names a live process ⇒ the child survived ⇒ adopt; `ESRCH` ⇒ it died across the recycle window ⇒ re-dispatch. (A transient unreaped zombie answers alive, but the immediate adopt + first read/`try_wait` observes the EOF/exit and drives the normal child-exit path, so it does not durably suppress a needed re-dispatch.) On non-unix there is no child-preserving recycle, so `child_survived()` is `true` (the adopt path is a no-op and nothing is ever severed mid-cycle).

3. **Idempotency.** Layered: a committed cycle is never open (so `boot_resume_action` returns `None`); a surviving child adopts without re-trigger (never double-runs); and the persisted `recycle_resume_consumed` latch (`cycle_state::mark_recycle_resume_consumed`) stops a SECOND boot reading the same still-open-but-child-dead checkpoint from re-dispatching the turn again.

4. **Deferred-too-long escalation counter** (`idle_watch.rs` + `cycle_open_defer_escalates` / `MAX_CYCLE_OPEN_DEFER_TICKS` in `decisions.rs`). The idle-watch tracks `cycle_open_defer_streak` — consecutive ticks the recycle has been deferred for an open cycle AT a turn boundary (off a boundary the recycle never fires, so an open cycle there is not starving anything and does not accrue the streak). Past `MAX_CYCLE_OPEN_DEFER_TICKS` (40 ticks ≈ 20s at the 500ms poll) the watch ESCALATES: it recomputes the recycle action with `effective_cycle_open = cycle_open && !escalate` = false, forcing the deferred recycle. The forced `execve` severs the never-closing/wedged cycle — but the open `#durablerecycle` checkpoint survives on disk, so the boot-resume path above re-dispatches the genuinely-interrupted turn. This guarantees a never-closing cycle cannot starve a stale-binary self-recycle or an operator restart indefinitely. Ops-log provenance: per-tick `supervisor_recycle_deferred_cycle_open ... defer_streak=N/M` and the one-shot `supervisor_recycle_cycle_open_escalated ... action=force_recycle reason=cycle_never_closed`.

**Test coverage (Phase B):**

- `decisions.rs` unit: `boot_resume_action` full truth table (re-dispatches ONLY when cycle open AND child died AND not consumed; adopts on surviving child; `None` on committed / not-recycle-boot / already-consumed); `cycle_open_defer_escalates` threshold (no escalation below the bound, escalates at/past it); `ReexecState::child_survived` distinguishes a live pid (self) from a reaped-child pid; the converse that clearing `cycle_open` restores `RecycleImmediate` (what the escalation relies on).
- `cycle_state.rs` unit: `mark_recycle_resume_consumed` latches once, is idempotent, does not close the cycle, and is `Ok(None)` with no state.
- SimWorld: `never_closing_cycle_escalates_recycle_then_boot_redispatches_interrupted_turn` (defers below the threshold, escalates + forces the recycle at the threshold even though the cycle never closed, child dies, boot re-dispatches once, a second boot does NOT re-dispatch) and `recycle_boot_with_surviving_child_adopts_without_redispatch` (the surviving-child idempotency path adopts without re-dispatching).

**Files (Phase B):**

- `agent-doc-orchestration/src/start/decisions.rs` — `BootResumeAction` + `boot_resume_action`, `MAX_CYCLE_OPEN_DEFER_TICKS` + `cycle_open_defer_escalates`, `ReexecState::child_survived`.
- `agent-doc-orchestration/src/cycle_state.rs` — `recycle_resume_consumed` field + `mark_recycle_resume_consumed`.
- `agent-doc-orchestration/src/start/run.rs` — the boot-resume path.
- `agent-doc-orchestration/src/start/idle_watch.rs` — the `cycle_open_defer_streak` escalation counter + `effective_cycle_open` gating + ops-log lines.
- `src/sim_world.rs` / `src/sim_world/engine.rs` — the never-closing-cycle escalation → boot re-dispatch + surviving-child adopt model and coverage.

## Test coverage (Phase A)

- `decisions.rs` unit: `supervisor_recycle_action(..., cycle_open=true)` returns `DeferCycleOpen` for the stale+auto+head_pending, stale+wedge, stale+explicit_admin, fresh+explicit_admin, and reexec_failed inputs that otherwise recycle; `cycle_open=false` preserves the existing matrix.
- SimWorld/integration: a recycle decision evaluated while `cycle_state.is_open()` (preflight_started) defers; once the cycle is `Committed` and `inflight=0`, the same inputs recycle — proving a recycle in the preflight→finalize window cannot fire and therefore cannot produce `live_prompt_drift_after_preflight`.

## Files

- `agent-doc-orchestration/src/start/decisions.rs` — `supervisor_recycle_action` + `SupervisorRecycleAction` (add `DeferCycleOpen`, `cycle_open` param).
- `agent-doc-orchestration/src/start/idle_watch.rs` — compute `cycle_open`, thread into the recycle + restart-drain-reexec decisions, gate `do_recycle`, log the deferral.
