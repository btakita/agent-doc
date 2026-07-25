# Plan: Migrate imperative coordination into Lazily state

Status: READY FOR REVIEW (implement in a fresh/cleared session). Owner: agent-doc dogfooding.
Survey: 4 parallel read-only subsystem passes (controller/supervisor, sync/route/tmux,
write/delivery/closeout, watch/ipc/agent-io), 2026-07-25. All findings grounded in
`file:line`. Coined tags are written without a leading `#` on purpose so they do not
resolve to phantom backlog items; file real ids when a cleared session queues each phase.

## Motivation

agent-doc's architecture rule (`AGENTS.md` §"lazily-hot-path", §"Durable effect
sinks") says every hot-path coordination decision — ownership, claims,
heartbeats/leases, compare-and-swap, deduplication, supersession, queue/closeout
serialization, liveness — must be an **actor transition against the in-memory
Lazily projection**, and durable storage (`state.db`) / `flock` is a **write-only
effect sink or actorless bootstrap boundary**, never a transition authority. Real
incidents keep tracing back to *imperative* coordination that violates this: a
poll loop or TTL that re-derives a decision from OS state / SQLite / a timer
instead of reacting to the one Lazily projection every other consumer reads.

Live examples from this session:

- The operator-visible **compact wedge** (`crdt_delivery_ack_pending`) — five
  separate loops independently poll or re-read the *same* delivery-convergence fact.
- **ghost-editor-liveness** (already fixed in commit `73e489aab`) — the exit
  watcher's private `watched` set drifted from the plane's open-set. The fix
  unioned them imperatively; the *coherent* form is a derived observable over the
  plane.
- **ghost-text-misread-as-draft** — route/dispatch read Claude Code's autosuggest
  ghost text ("yes, compact it") as unsent operator input and refused to inject
  (`blocked_with_exact_unblocker`), when the composer was effectively empty.

North-star patterns already in-tree:
- `EditorReplicaLivenessWitness` (`agent-doc-document-realtime-io/src/lib.rs`
  ~5156-5266): replaced a TTL ("did this fail recently?") with a **Lazily witness**
  ("has anything changed since it failed?"). Suppression tracks the fact, not a clock.
- `WaitMachine` (`agent-doc-turn/src/wait_machine.rs`): a pure, Lean-proven
  transition `tick` with the *single* auditable `sleep` isolated in the driver.
- `command_plane.rs`: guarded by an in-source architecture test (`rpc.rs:15733`)
  forbidding `flock`/`open_state_db`/`state_store::load` on the authority surface —
  proof the boundary is enforceable.

## Classification rubric

1. **Hot-path coordination decision?** (ownership / liveness / lease / dedup /
   queue / closeout / CAS / supersession) → must be a Lazily transition or a
   reactive read of the projection.
2. **Effect sink vs authority.** SQLite/flock/timer as a *write-only sink /
   bootstrap* = OK; as a *decision arbiter* = violation → migrate.
3. **Genuine OS boundary?** `kill(pid,0)`, tmux `capture-pane`, file mtime,
   subprocess stdout, PTY IO have no reactive primitive → keep the poll as an
   **effect-source**, but **publish the result once** to a Lazily cell all
   consumers react to (never re-poll per consumer).
4. **TTL that guesses?** → replace with a Lazily **witness** (edit epoch,
   registration set, phase).

## Target Lazily primitives

| Need | Primitive |
|------|-----------|
| Derived coordination value read on the hot path | reactive **derived cell** (`computed(f)`) |
| React to a state change with a side effect (reap, notify, checkpoint, retry) | **Effect** / **AsyncEffect** gated on a signal — never a poll loop |
| Ordered, lossless cross-process fact stream | **TopicCell** / **Outbox** (`ack_through`) + epoch supersession |
| Reactive membership (open editors, watched pids, active sessions) | reactive **observable set** |
| "Has the fact changed since?" suppression | **witness** compared by equality |
| Latest-projection persistence | idempotent upsert from an Effect (batch A→B→C persists only C) |

Non-goal: removing genuine OS-boundary polls. Goal: their *results* flow through
the single Lazily projection so no consumer re-derives coordination from a private
timer/set/SQLite read.

## Unifying wins (how the findings collapse onto ~6 primitives)

### A. One `delivery_converged` observable cell per file  — highest leverage
Fact producer exists: `delivery_converged_for_file`
(`agent-doc-crdt-relay-io/src/lib.rs:3038`) / `CrdtDeliveryAckPending`
(`agent-doc-state-backbone/src/lib.rs:73`). Expose as one Lazily cell; gate Effects
on it. **Directly dissolves the compact/delivery-ACK wedge class.** Collapses:

> **DESIGN CORRECTIONS (2026-07-25 investigation, before implementing Phase 1).**
> A read-only survey of the actual code overturns three premises in this section —
> do not start Phase 1 from the bullets below without folding these in:
> 1. **There is no cell to "await" at `:492`.** `write-converge-io/src/lib.rs:492`
>    is the enum *variant* `IpcSnapshotSource::LazilyVisibleWriteEvent`, not a Lazily
>    cell. The visible-write receipt is a **per-call durable projection lookup**
>    (`visible_write_content_from_lazily_event` → `visible_write_commit_candidate_for_patch_file`
>    → state-backbone projection), recomputed each poll. W1 sleep-polls *because* no
>    notify/cell exists. The cell must be **built**, not awaited.
> 2. **W1 and `delivery_converged` are two DISTINCT facts, not one.** W1 polls the
>    *visible-write receipt for `(file, patch_id)`*; `delivery_converged_for_file` is a
>    fold over `RelayHub.members[*].pending` (all live replicas ACKed). Theme A is
>    really *two* observables (receipt-arrived, replicas-converged) that happen to
>    gate the same closeouts — the "one cell" framing is too coarse.
> 3. **`WaitMachine` is the WRONG driver for W1.** It hard-caps every non-exempt
>    await at `GLOBAL_HANG_CEILING = 10s`, but the receipt legitimately needs to wait
>    far longer on a slow controller (`CONTROLLER_CRDT_CURRENT_TEXT_TIMEOUT ≥ 120s`
>    lives right next to it). Routing W1 through `WaitMachine` would fire premature
>    timeouts = a convergence regression / fresh wedge. `lazily` exposes **no**
>    synchronous caller-bounded await-on-change primitive (only push `Cell::subscribe`
>    inside the graph, and `ThreadSafeContext`'s internal `wait_for_recompute`
>    condvar). Phase 1 must **build** a caller-bounded, condvar-notified
>    `await_visible_write_receipt(file, patch_id, deadline)` whose deadline is the
>    caller's real timeout (not a 10s global clamp). Keep the fold/projection read
>    authoritative in the wait predicate so a missed notify degrades to a slower
>    poll, never a wedge.
>    **Cross-process caveat (decisive):** the receipt is normally recorded by the
>    *controller* process (`record_visible_write_commit_candidate_direct`,
>    rpc.rs:10875) while the waiter runs in the *CLI* process (preflight-runtime /
>    write-ipc / write-runtime / compact). An in-process `Condvar` cannot wake a
>    waiter in another process, so the naive "writer bumps a condvar" primitive is
>    near-worthless here — the CLI that just recorded its own receipt would find it
>    on the next predicate check anyway. The real primitive must be a **cross-process
>    push**: either the controller emits a `visible_write_receipt(patch_id)` event
>    the client subscribes to (extend the existing controller→client IPC / the
>    reliable-sync push plane), or the receipt fact rides the reliable-sync outbox
>    that clients already tail. Do NOT ship an in-process condvar and call W1 done.
> 4. For `delivery_converged` (W6/W8/compact), making it reactive means threading a
>    monotonic mutation version through `RelayHub`'s `members`/`pending`/liveness
>    writes and exposing a `DeliveryConvergenceWitness { version, converged }`
>    (the `EditorReplicaLivenessWitness` idiom), with the `delivery_converged()` fold
>    staying authoritative in the probe. **W7 is not a `delivery_converged` consumer**
>    at all: it waits for the editor's native `save_document` flush to hit *disk* —
>    a genuine OS boundary (rubric #3), so publish the flush result once and gate on
>    it, don't fold it into the convergence cell.
>
> Net: Phase 1 is a build-new-primitive cycle on the highest-blast-radius surface
> (the operator-visible `crdt_delivery_ack_pending` wedge), not a mechanical
> await-swap. Sequence it as its own focused session per the phase note below.

- **W1** visible-write (delivery-ACK) receipt poll loop — `write-converge-io/src/lib.rs:1667` (`poll_visible_write_content_lazily_event`, wrapper `:1719`, callers `:397`); `LazilyVisibleWriteEvent` cell exists (`:492`) → await it, don't spin `DeadlineCore` (`:1698`). THE convergence wait. M/med.
- **W3** preflight pre-mutation settle loop — `preflight-io/src/debounce.rs:100` (loop `:117`, twin `:188`). M/med.
- **W6** compact commit-observe retry (disk+snapshot re-read) — `compact-io/src/lib.rs:696`. M/med.
- **W7** compact editor-buffer flush poll (`save_document` typed ACK exists `:1001` but discarded for disk polling) — `compact-io/src/lib.rs:1003` → AsyncEffect awaiting the ACK. S-M/low.
- **W8** compact CRDT-merge converge retry — `compact-io/src/lib.rs:1208`. Fold in. M/med.
- **W4** queue-authority observation retry — `preflight-io/src/lib.rs:157` (backoff `:238`); keep `decide_authority_recovery` as the pure transition. M/med.

### B. Liveness plane as the single ownership/liveness authority — unblocks the most
Route the supervisor-lease heartbeat and pane/pid liveness through the same Lazily
liveness plane the editor watcher already feeds. Then every ownership/GC/reap
decision reads the plane instead of re-probing. Collapses:
- **C1** `fresh_foreign_supervisor_lease_holds_document` — SQLite lease read + 60s TTL + `kill(pid,0)` arbitrating cross-document ownership — `project_controller.rs:1745` (window `:1715`). Largest ownership inversion. L/high.
- **C5** controller GC stale-actor sweeps — SQLite actor store + TTL + OS liveness — `project_controller.rs:1774` / `:1872` / `:1985`. L/med-high.
- **C7-liveness** supervisor self-kill `process_is_alive` poll — `supervisor-io/src/selfkill.rs:209` (loop `:227`). Publish liveness to plane; keep SIGTERM→SIGKILL escalation as sink.
- **S5** startup readiness verdict re-polled by many consumers — `route-io/src/startup_ready.rs:77` (poll `:91`, interval `:14`). Keep tmux capture; publish `pane_ready(pane_id)` observable. M/med.
- **D8** IPC `is_listener_active_for_pid` connect-probe instead of plane read — `ipc-io/src/lib.rs:178`. M/med.
- **C8** `process_exit_watcher::run_poll_loop` (`:78`) — `kill(pid,0)` correctly kept & published once; make the `watched`-set union (`:91-92`) a **derived observable** over `plane.all_open_pids().filter(pid_alive)`, deleting the `watched` HashSet and the ghost-editor-liveness drift class entirely. S-M/low.
- Also `session_liveness.rs:51` `pane_owns_live_agent` (tmux `#{pane_current_command}`) — OS boundary; publish once to plane instead of re-polling per consumer.

### C. Document edit-epoch signal + live turn-phase cell
The watch daemon debounce/loop-prevention/convergence trio all want the signal
reliable-sync already publishes (`plane_document_in_flight_for_path`,
`reliable-sync-io/src/lib.rs:84`; `edit_epoch`/`synced_epoch`, `liveness.rs:295`).
Collapses:
- **D3** `cycle_freshly_in_flight` reads durable cycle-state (SQLite) at the seam — `watch-io/src/daemon.rs:673` / `:838`, TTL `:879`. Clearest inversion in watch. M/med-L.
- **D1** file-change debounce fixed-`Duration` guess — `daemon.rs:601`; generalize the `WatchEntry.reactive` zero-debounce path (`:187`) to settle on edit-epoch quiescence. M/med.
- **D2** `is_agent_change` wall-clock loop-prevention window — `daemon.rs:622` → derived cell on last agent-emitted epoch. M/med.
- **D4** convergence check re-reads file+hash — `daemon.rs:643` → derive from the doc cell's content-epoch. S-M/med.
- Feeds **S2**.

### D. Owner-pane witness derived cell
Pane-ownership is re-derived independently at ~15 seams; the per-run
`SyncProofCache` / observation-scope guards (`sync.rs:2065-2072`,
`tmux-io/src/observation_cache.rs`) are a hand-rolled memo of exactly this.
- **S3** `find_normal_path_owner_pane` / `find_live_owner_pane` — `sync.rs:4317` / `:4306` / core `:4339`; consumers across `route-io`, `focus-io`, `resync.rs`, `dispatch_target.rs`, `launch_contract.rs`, `restart_handoff.rs`, `startup.rs` → publish owner witness once per `(document_id, session_id)` into a derived cell; invalidation becomes dependency-driven. L/high.
- Reference (leave): **S4** `authoritative_actor_binding` (`rpc.rs:682`, cold fallback `:726`) — already controller-RPC-to-live-actor; only smell is being re-observed per caller (feeds S3).

### E. TopicCell/Outbox for cross-process request channels + epoch supersession
- **C6** cross-supervisor recycle request — `supervisor-io/src/recycle_request.rs:77` / `:83` (TTL `recycle_request_is_fresh`). M/med.
- **C7-channel** supervisor self-kill file-sentinel polled per idle tick — `selfkill.rs:77`. M/med.
- **C3** startup-miss dedup/supersession from ledger + epoch CAS — `supervisor-io/src/startup_miss.rs:184` / `:240` / `:313`. Note the ledger-scope thread-local cache (`state_events.rs:50`) papers over "995ms of a 1024ms ownership loop" — coherence + latency win. M/med.
- **C4** `record_starting_actor_timeout` dedup from ledger + epoch CAS — `controller-io/src/starting_actor_timeout.rs:38` (caller already holds `facts`). S-M/low.

### F. Retained-capture resume as a phase-transition cell
- **W2** `try_resume_captured_finalize_in_hook` imperative retry loop (`MAX_ATTEMPTS=4`, backoff) — `codex-stop-io/src/lib.rs:272` (loop `:280`, sleep `:317`) → drive from a Lazily response-cell phase (`retained → converging → committed/needs_operator`); `Retryable` becomes an internal actor transition. Ties to Theme A. L/med.

### G. `sync.lock` as a Lazily layout-ownership lease
- **S1** `acquire_sync_lock` flock contention busy-wait + `/proc` stale-owner reap — `sync-io/src/lib.rs:105` (poll `:140`), driver `sync.rs:2034`, stale-reap `lib.rs:620` → layout-ownership **lease** keyed by `scope_root`; stale-orphan reap becomes lease expiry/heartbeat. Keep flock only for genuinely actorless bootstrap. L/high.
- **S2** `rename_debounce` coordination-lease TTL — `sync.rs:562`, direct SQLite `SELECT` + 5s TTL bypassing the controller RPC. Cheapest high-value durable-sink fix → witness keyed by document-state-hash; route through the controller actor. M/med.

### H. Retry belongs to the plane that can dedupe it — not the call site
*(added 2026-07-25 after the `document_state_projection` EAGAIN incident, commit `afb0ed132`)*

Same shape as the durable-sink rule, one level up: **a transport may retry only
what the protocol can deduplicate.** Retry-safety is a property of the *plane*,
not of the call site, so a per-caller retry knob is always the wrong layer.

The library half already exists and agent-doc uses **none** of it:
- `lazily-rs/src/resilience.rs` (`#lzresilience`) — `RetryPolicyCell`/`Core`
  (exponential backoff `min(cap, base·2^attempt)`, reactive `delay_cell()`),
  `CircuitBreakerCell` (Closed/Open/HalfOpen over a sliding failure window). Its
  own header says it composes with `CommandTransport`.
- `lazily-rs/src/command.rs` (`command-plane-v1`) supplies retry's precondition:
  `idempotency_key`, `DedupePolicy::{SameIdempotencyKey, SameCommandId}`,
  `supersede`, and the hard rule *"terminal authority is the causal receipt, not
  the event or the transport"* — a unary `call` resolves only on a terminal
  projection, never on an ACK.
- `lazily-rs/src/spill.rs` — `ack_through` + `spill_replay_idempotent`: for an
  idempotent policy, replay-from-cursor **is** retry.
- Grep proof: the only `resilience` hit in `src/agent-doc` is an unrelated
  `crash_resilience::install()`.

The inversion: `project_controller/rpc.rs:209` `request_path_json` is a bare
NDJSON request/response with **no idempotency key**, so a recv-timeout is
indistinguishable from "applied, answered slowly." `retry_controller_transport_drop`
(`:274`) can therefore only retry clean transport *drops* (reset/abort/EOF/closed),
and `read_controller_response_line_with_timeout` (`:233`) must bail. Adding a
retry there would risk double-applying a mutating command; the honest options are
only *fail closed* (today) or *make it idempotent first*.

- **R1** Route timeout-prone controller reads through the command plane that
  already carries `idempotency_key`, and gate the transport with
  `RetryPolicyCell` + `CircuitBreakerCell` instead of the bespoke
  drop-only retry. Start with **idempotent reads** (`actor_binding`,
  `document_state_projection`) where double-apply is meaningless. M/med.
- **R2** Guardrail (mirrors the `command_plane.rs` architecture test): assert no
  retry wrapper is attached to a request path lacking an `idempotency_key`. S/low.
- Interim (landed `afb0ed132`): the state-wire transport classifies a recv-timeout
  as a distinct `ActorRequestError::Timeout` so *read* callers degrade to the cold
  projection while hot-path writes still fail closed. That is the fail-closed half;
  R1 is the principled half.

## Leave-as-is (reference patterns / genuine OS boundaries)

- `WaitMachine`, `closeout_recovery.rs`, `document_drift.rs` — pure transitions; the reactive Effects above should *call* these.
- `authoritative_actor_binding` (S4), `command_plane.rs` (already guarded).
- OS/PTY/socket boundaries: `process_is_live`, tmux `capture-pane`, `ipc-io` accept/receipt timeouts, `shared_writer.rs` PTY write, `agent/*` subprocess drains, `prompt-io` keystroke pacing, `resize.rs`/`io_threads.rs`. Keep as effect-source; publish once.
- **W9** callback ledger TTL expiry (`callback-io/src/lib.rs:168`) — keep the ledger as sink; route only the `is_expired` transition through the callback actor. S/low.
- Watch **D5** registry rescan (`daemon.rs:439`) → observable set (M, lower urgency); **D6** stream-capture tmux poll (`daemon.rs:532`) publish-once (S, low); **D7** idle-timeout shutdown (`daemon.rs:424`) optional (S, low).
- Startup **S6** flock bootstrap locks (`route-io/src/startup_locks.rs`) — legit actorless bootstrap; design-review, likely leave. **S7** registry `load_actor_record` gate (`registration.rs:504`) — audit it is cold hydration; else route through controller.

## Adjacent gaps surfaced this session (track separately, not Lazily-state items)

- **ghost-text-misread-as-draft. — RESOLVED 2026-07-25.** route/dispatch treated
  Claude Code autosuggest ghost text as an unsent operator draft and refused
  `route --dispatch-only` injection (`blocked_with_exact_unblocker`,
  `unblocker=submit_or_clear_pane_draft`) when the composer was actually empty.
  Commit `d9e306c38` covered only the *static* fresh-session hint via an
  allow-list; this plan correctly flagged that it missed a **populated**
  autosuggest. Fixed generally: `pane_composer_draft` (`agent-doc-harness`) now
  rejects any candidate whose composer body renders **dim/faint** (SGR `2`) in the
  raw capture, via the harness-agnostic
  `prompt::prompt_candidate_is_dim_placeholder` (the existing Codex dim scanner,
  which already accepted `>`/`›`/`❯`, lifted out from behind its `codex_` name).
  Styling — not wording — is the discriminator, which is what generalizes: the
  autosuggest text is dynamic, so no allow-list can enumerate it. Real
  (normal-intensity) input at the same prompt is still protected as a draft;
  both directions are pinned by
  `dim_autosuggest_ghost_text_is_not_an_operator_draft`.
- **Controller relaunch passes an empty project root (`ctrlrespawnenoent2` guard
  reproduced 2026-07-25).** After `make install`, the running controller self-recycles
  off the replaced binary — and the automatic relaunch then fails closed for the whole
  project: every controller-dependent command (`agent-doc read`, `admin inspect`)
  errors with `cannot launch project controller: project root "" is empty or not a
  directory`, while `admin list` (durable-state only) keeps working. Recovery was a
  manual `agent-doc controller serve --project-root <root>`. The guard at
  `project_controller/rpc.rs:7174` already names the shape correctly — a caller is
  handing the launcher an empty root — so the open question is only *which* caller.
  `project_root_or_cwd` (`agent-doc-project-root-io/src/lib.rs:21`) returns an
  explicitly-passed root **without validating it is non-empty**, so a
  `Some(PathBuf::new())` anywhere upstream propagates straight through to
  `Command::current_dir`. Repro: kill the project controller, then run any
  controller-dependent command. This is install-triggered, so an install-heavy
  dogfooding session self-inflicts it — it is why a project can go dead right after a
  fix lands. Reproduced three times on 2026-07-25, once per `make install`; each
  needed a manual `controller serve` to clear, and one occurrence broke a
  *concurrent* operator session's Run Agent Doc (`command-plane editor_route request
  failed via .agent-doc/controller.sock: No such file or directory`).

  **Do not "fix" this by hardening `project_root_or_cwd` alone — that is unproven.**
  `admin inspect` resolves its root via `project_root_for_target_or_cwd(None,
  Some(document))`, which should already yield the real root, so the empty value may
  instead come from a stored bootstrap/status record consulted during relaunch. One
  discriminating experiment settles it: with the controller down, run
  `agent-doc admin inspect --project-root <explicit root> <FILE>`. Still failing with
  the empty root exonerates the resolver and points at persisted launch state;
  succeeding means the resolver seam is the cause and rejecting an empty explicit
  root there is the fix. Not a Lazily-state item.
- External `tmux send-keys Enter/C-m` cannot submit a route-owned pane's Claude
  composer (supervisor runs the harness in an inner PTY); operator recourse is
  manual keyboard — undercuts unattended recovery. (Mostly moot given the item
  above: there was nothing to submit.)

## Phased migration

Each phase: split functionality + tests together; SimWorld / deterministic
decision fn over live-timing mocks; keep pure transitions (WaitMachine-style)
separate from the reactive Effect wrapping them; `make check` green; one focused
backlog id per phase; do not touch fixed-seed fuzz.

- **Phase 0 — Groundwork (DONE).** ghost-editor-liveness reap + honest
  recovery diagnostics landed (`73e489aab`). C8 derived-observable cleanup landed
  (`fd13909ab`): `OsProcessExitWatcher` is now stateless — the `watched` HashSet is
  deleted and the poll loop derives its candidate set solely from the liveness
  plane's `all_open_pids().filter(pid_alive)` (a proven superset of any private
  `watch()` tally, since `mark_editor_attach_open` only attaches plane-sourced pids).
  A zero-sized-struct regression guard forbids a private tally from returning. This
  is the pattern exemplar for the phases below.
- **Phase 1 — Theme A: `delivery_converged` cell. W1 half LANDED; rest open.**
  Highest leverage; dissolves the compact/delivery-ACK wedge (W1, W3, W6, W7, W8, W4).

  **Landed 2026-07-25 — the cross-process receipt push (W1).** The missing primitive
  was built as design correction 3 required, and it is *not* an in-process condvar.
  Findings that shaped it: the receipt is recorded through the controller
  (`visible_write_commit_candidate_observed` → `append_apply_state_event`) and read
  back through it (`..._patch_status` → in-memory projection), so the controller is
  already the one process holding both sides — the only missing piece was the
  **await**. Its fallback read (`load_state_backbone_projection`) folds the *entire*
  state-event ledger, so every poll iteration of W1 was an O(all events) fold against
  a 50MB `state.db`; that, repeated for up to 120s, is the wedge's cost.
  - `ControllerRuntime::wait_for_visible_write_commit_candidate_patch` — bounded,
    condvar-notified await mirroring the in-tree `wait_for_supervisor_recycle_settle`
    idiom, parked on a new `state_projection_waiters` notified by `apply_state_event`
    / `refresh_memory`. The in-memory projection stays authoritative and is re-read on
    every wake, so a missed notify degrades to a slower wait, never a wedge.
  - Controller command `visible_write_commit_candidate_patch_await` takes the
    **caller's real deadline** (`wait_ms`, clamped to a 120s controller ceiling, not a
    10s global hang budget — design correction 3), and answers the identical proof
    shape as `..._patch_status`. Safe to block: the controller already serves each
    connection on its own thread, and `serve_client`'s idle timeout covers only
    reading the request, not the handler.
  - `poll_visible_write_content_lazily_event` now parks on that push for the rest of
    its deadline instead of sleeping on a private timer; an absent controller or a
    failed await logs `visible_write_receipt_await_unavailable` and falls back to the
    old poll cadence.
  - Coverage: 4 regressions incl. wake-on-append asserted by elapsed time against a
    30s deadline (a silent degradation to poll-until-deadline still returns the
    receipt, so time is the only honest assertion), plus a carry-forward guardrail
    asserting the decision site never reads `open_state_db` /
    `load_state_backbone_projection` / `load_state_event_ledger` / `flock`. The
    guardrail was negative-tested — it fails when a violation is injected.

  **Landed 2026-07-25 — `DeliveryConvergenceWitness` (the Theme A witness itself).**
  `RelayHub` gained a `delivery_epoch: Source<u64>` bumped at every write that can
  change the fold — the 5 `pending` mutation sites, liveness transitions (`set_live`),
  and membership (`bump_membership_epoch`) — and
  `RelayHub::delivery_convergence_witness() -> DeliveryConvergenceWitness { version,
  converged }`. It follows the hub's existing `#live-editor-reactive` idiom
  (`ThreadSafeCellMap` liveness + `membership_epoch` + `Computed` count) rather than
  inventing a mechanism, and the `delivery_converged()` fold stays authoritative as
  design correction 4 requires. Note the real mutation surface was **5 sites**, not
  the broad `members`/`pending`/liveness sweep the plan implied.
  - Coverage includes the property that actually makes a witness useful:
    `delivery_convergence_witness_version_is_stable_while_nothing_changes` — a version
    that moved on every read would be "correct" and suppress nothing.
  - First consumer: `crdt_current_text[_unavailable]` now logs `delivery_version`
    beside `delivery_converged`. During a wedge, a repeated `delivery_converged=false`
    cannot distinguish *churning but never settling* from *nothing moving at all*; a
    frozen version says the latter outright.

  **⚠ Correction for the remaining Phase 1 work — the hub is PROCESS-LOCAL.**
  `hub_registry()` (`agent-doc-crdt-relay-io/src/lib.rs:531`) is a process-local
  static, and `delivery_converged_for_file` defaults to **`true`** when this process
  hosts no hub. So W6/W8 — which run in the *compact CLI* process — cannot consume the
  witness directly: they would read an absent hub and be told "converged". The
  remaining half therefore needs the **same cross-process treatment as W1** (a
  controller-side await command over the controller's hub), not a local call. This is
  why `delivery_convergence_witness_for_file` returns `Option` instead of defaulting:
  a suppression key must never be invented for a document the caller cannot observe.

  **Landed 2026-07-25 — the cross-process convergence await + W6.** The correction
  above is now implemented: `delivery_convergence_await` publishes convergence once
  from the process that owns the hub (a single in-memory poll there replacing N
  filesystem re-reads in each CLI — rubric #3), and `DeliveryConvergenceStatus`
  carries an explicit `observed` flag so a controller hosting no hub for the document
  says so immediately instead of waiting out the deadline or claiming convergence.
  W6 (compact commit-observe) now waits on that fact for its backoff window rather
  than sleeping it blindly, and falls back to the previous blind sleep whenever no
  controller or hub can answer, so the loop stays fail-open.

  **Still open in Phase 1:** W8 (compact CRDT-merge converge retry) can now consume
  the same await; W7 needs the editor `save_document` disk-flush result
  published once (a genuine OS boundary, rubric #3 — do not fold it into the
  convergence cell, and note its current 1s/20ms disk poll is already the
  single-consumer effect-source shape, so the win there is small); W3 and W4 follow
  once the cross-process convergence await exists.
  **⚠ Read the "DESIGN CORRECTIONS" block in Theme A first.** The 2026-07-25
  investigation overturned this section's premises: there is no cell to await (it
  must be built), W1 polls a *different* fact than `delivery_converged`, `WaitMachine`
  is unsafe here (10s clamp vs. ≥120s legitimate waits), and the receipt is
  cross-process (in-process condvar is worthless — needs a controller→client push).
  This is a **build-new-cross-process-primitive cycle on the highest-blast-radius
  surface** (operator-visible `crdt_delivery_ack_pending` wedge), not an await-swap;
  give it its own focused session with SimWorld coverage before touching the five
  hot-path loops.
- **Phase 2 — Theme B: liveness plane as ownership authority.** Route
  supervisor-lease heartbeat + pane/pid liveness through the plane; convert C1, C5,
  C7-liveness, S5, D8 to plane reads; land C8 fully.
- **Phase 3 — Theme C: edit-epoch + turn-phase cell.** Watch daemon trio (D1-D4) +
  S2 rename-debounce onto the shared signal.
- **Phase 4 — Theme D + G: ownership seams.** Owner-pane witness cell (S3) and
  `sync.lock` layout lease (S1) — broad blast radius, after B lands the plane.
- **Phase 5 — Theme E + F + cleanups.** TopicCell request channels (C3, C4, C6,
  C7-channel), retained-capture phase cell (W2), then D5/D6/D7/W9/S6/S7 audits.
- **Phase 6 — Theme H: retry onto the dedupe-capable plane.** R1 (idempotent
  controller reads through the command plane, gated by `RetryPolicyCell` +
  `CircuitBreakerCell`) then R2 (guardrail). Independent of Phases 1-5 — it
  touches the transport seam, not the state seam — so it can be sequenced
  wherever it fits; do R2 in the same cycle as R1 so the boundary cannot regress.

## Guardrail (carry forward)
Extend the `command_plane.rs` in-source architecture test (`rpc.rs:15733`) to each
migrated seam: assert the decision site does not `open_state_db` /
`state_store::load` / `flock` once its Lazily cell exists. That test keeps the
boundary from regressing.
