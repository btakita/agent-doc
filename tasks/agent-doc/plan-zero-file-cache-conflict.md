# Plan: Zero JB `File Cache Conflict` dialogs

## Operator goal (2026-06-12)

> "I'm getting JB `File Cache Conflict` dialogs. Please fix. We should have 0
> `File Cache Conflict` dialogs."

## Root cause (confirmed in code)

A JB `File Cache Conflict` dialog fires whenever agent-doc writes the **document
file to disk** while IntelliJ holds an unsynced (dirty) in-memory buffer for it.

`#w42v` (`try_compact_editor_converge`) fixed exactly **one** path — `compact` —
by converging the compacted document through the editor via a component-scoped
`op:replace` IPC patch, keeping the disk write only as `transport=disk_fallback`
when no listener is active. **Every other document-mutating write still goes
straight to disk** via `atomic_write(file, …)`:

- `write.rs:3186` queue consume (`queue consume: failed to write document`)
- `write.rs:3887` queue done-id mark
- `write.rs:5014`, `5160` repair / rewrite paths
- `write.rs:8683`, `8948`, `9488`, `9836`, `10240`, `10283`, `10367`, `10743`
  finalize / stream / fallback write paths
- `reset --from-current`, post-commit working-tree repair

When the IDE buffer is dirty (operator typing, or the supervisor's stale buffer),
any of these diverge from the buffer → `File Cache Conflict`. This is the **same
IPC-buffer-divergence class** as the recurring `live_prompt_drift_after_preflight`
/ exit-75 wedge (`#ipcdrift` / `#stuckhandoff2`) that forces disk fallbacks in the
first place.

## Fix shape

Route **all** document-mutating writes through one editor-IPC convergence gate,
mirroring `#w42v` and the FlowCore `closeout` / `document_mutation` contract
(Phase 3): "IPC, stream, direct write, finalize, and repair all use the same
terminal-state guard; converge editor/disk/snapshot." When a listener is active,
converge through the editor (`op:replace` component patches) and verify ack
content; only disk-write when no listener (`transport=disk_fallback`). Plus the
underlying `#ipcdrift`/`#stuckhandoff2` wedge fix so IPC stops failing and forcing
the fallback.

## Phases (each clean-session; live verify is operator-drive)

- **#fcc0a** ✅ DONE — Inventoried + classified every `atomic_write(file, …)`
  document-write site in `write.rs`. Finding (sharper than the original framing):
  - **Genuine direct-disk-no-IPC component writes** (the dialog sources that never
    even tried the editor): queue consume, queue done-id mark, and `run_template`
    (non-CRDT template mode). These are the ones the converger fixes.
  - **Already-post-IPC fallbacks** (CRDT `run_stream` finalize/stream/IPC-timeout,
    `live_prompt_drift` commit auto-recovery, repair materialization): these reach
    `atomic_write` only AFTER `try_ipc` / editor convergence already failed, so
    routing them through the converger just re-hits the same dead listener. Their
    dialogs are the `#ipcdrift`/`#stuckhandoff2` wedge → tracked as **#fcc0e**.
  - **Inline mode** (`run`, `## User`/`## Assistant` blocks): no `agent:` components,
    so component `op:replace` convergence is N/A (would need the disabled
    `try_ipc_full_content`); left as a guarded disk write.
  - Sidecar/patch/marker writes (`.agent-doc/…`) are not document writes — excluded.
- **#fcc0b** ✅ DONE (`612a8dae`) — Generalized the `#w42v`
  `try_compact_editor_converge` into `try_editor_converge(file, target, current,
  source)` with a `source`-labelled log line (`<source>_writeback …
  transport=editor_ipc|disk_fallback`), and added the shared
  `converge_document_or_disk(file, target, current, source)` gate. Unit coverage:
  listener-active → `transport=editor_ipc` (source-labelled, exchange/queue scoped,
  no clobber); no-listener → `transport=disk_fallback` writes the target.
- **#fcc0c** ✅ DONE (`612a8dae`) — Migrated queue consume + done-id mark (the
  highest-frequency operator paths) onto `converge_document_or_disk`. The
  force-disk repair path keeps its raw `atomic_write` bypass (intentionally skips
  IPC/IDE + the visible-write guard).
- **#fcc0d** ✅ DONE (`612a8dae` + `9f0c63f8`) — Every genuine direct-disk-no-IPC
  component write now converges through the editor: `run_template` (non-CRDT,
  `612a8dae`) and `apply_template_from_string` (repair recovery, `9f0c63f8`). Each
  converges first / bare-disk fallback (the guard already ran). The remaining
  `atomic_write(file, …)` document sites are deliberately left direct, audited:
  - `run_stream` exit(75) (9541) + `run_ipc_timeout_fallback` (10293):
    post-`try_ipc`-timeout fallbacks — the listener just timed out, so socket
    re-convergence is provably wasteful. Their dialogs are the wedge → **#fcc0e**.
  - `run_stream` tail (9889): `force_disk` / no-plugin — force_disk must bypass IPC
    by contract; no-plugin has no listener.
  - inline `run` (8729) + `apply_append_from_string` (10336): inline
    `## User`/`## Assistant` docs have no `agent:` components → component
    `op:replace` convergence is N/A.
  - `try_auto_recover_live_prompt_drift` (7373) + `repair_disk_from_{normalization_fallback,
    ipc_dedupe}` (10796/12145): already editor-first — callers try
    `redeliver_*_to_editor` and fall to disk only when redelivery didn't land.
  - `repair_partial_response_materialization_before_fallback` (5026): IPC-degraded
    pre-fallback cleanup of a mid-IPC partial write; not a clean current→target delta.
- **#fcc0e** ✅ DONE (code) — Resolve the `#ipcdrift`/`#stuckhandoff2` wedge so IPC
  stops failing. All sub-surfaces landed; only the live JB proof remains:
  - **Wedged `--handoff-state preparing` controllers (#stuckhandoff2)** ✅ — M1-M5
    all landed (`08dfc27b`/`2b643683`/`3e0503ec`): controller self-watchdog suicide
    timer, orphan + cross-project `/proc`-scan reapers, client handoff drop-guard,
    and gate-dispatch-admission-on-`Stable`. See `plan-stuck-handoff-hardening.md`.
  - **Stale `patch_id` replay (#ipcdrift)** ✅ — claimed-patches sentinel (binary
    writes on disk fallback; JB + VS Code plugins consume before applying),
    generation fence, patch_id dedup, and the 08b read-only WatchService demotion
    (`#dsqa`) so the plugin's autonomous watcher no longer applies stale file-IPC
    patches — controller-owned watcher + socket IPC are the sole writers.
  - **Converger ↔ de-wedge integration** ✅ (`c7054e00`) — the `#fcc0` converger
    now respects the degraded latch (skips a wedged socket → `reason=listener_degraded`),
    feeds `record_ipc_socket_ack_timeout` on ack-timeout, and clears on success, so
    queue/template/compact converges are coherent with the circuit breaker.
  - **M2 worktree-write gate** ✅ — resolved after the live Phase-0 re-baseline
    (operator `go`, 2026-06-12). Investigation proved the controller has **no
    direct autonomous worktree-write path**: every controller-originated worktree
    mutation — external/operator dispatch AND the supervisor's self-driving queue
    drain (`command_kind=idle_queue_continuation`) — routes through `handle_dispatch`,
    which the M2 dispatch-admission gate (`3e0503ec`) already refuses on a non-Stable
    controller. The remaining `atomic_write` worktree writes run in the **agent CLI
    finalize** process, where gating on the *controller's* `handoff_state` would be
    both wrong (a legitimate finalize must not be blocked by a controller mid-handoff)
    and the contested `#pcwc` surface — so no separate write_queue gate is added.
    Proven with `autonomous_idle_queue_continuation_refused_when_controller_not_stable`
    (`c7054e00`-follow-up): the autonomous driver is refused + logged on a Preparing
    controller, admitted on a Stable one. Combined with M1's self-watchdog bounding
    the Preparing window, the wedge cannot corrupt the worktree.
  - **#pcwc Phase-2 post-commit worktree auto-reconcile** ✅ — `emit_postcommit_worktree_check`
    (`git.rs`) auto-reconciles the worktree to HEAD on the corruption class
    (`postcommit_worktree_lost_committed_content` — HEAD content dropped, no new user
    work) and pushes the committed blob back through editor IPC, while the carry-forward
    discriminator leaves a legitimate carried-forward superset untouched (the deliberate
    invariant). The old manual `git checkout HEAD -- FILE` recovery is now automatic.

- **#fcc0f** ✅ DONE (code) — **Non-`write.rs` document writes.** The `#fcc0a`
  inventory was scoped to `write.rs` `atomic_write(file, …)` sites and never
  inventoried the `std::fs::write(file, …)` document writes in the *other*
  orchestration modules, which bypassed the converger entirely and fired the
  dialog whenever the buffer was dirty. Audited every such site and routed the
  genuine component/frontmatter mutations through a new unguarded converge gate
  `converge_or_disk_write(file, current, target, source)` (in `write/converge.rs`,
  next to `converge_document_or_disk`; same `try_editor_converge` primary, but a
  **plain** `std::fs::write` fallback so the no-listener path stays byte-identical
  to the prior bare write — strictly a no-op without a live IDE, a fix with one):
  - `pending_cmd.rs` — all 23 `agent:pending`/`agent:review` operator-CLI ops
    (`add`/`done`/`gate`/`ungate`/`edit`/`clear`/`reorder`/`reap`/`review-*`/…),
    source `pending_write`.
  - `dedupe.rs` — `dedupe` command, source `dedupe`.
  - `preflight/maintenance.rs` — per-cycle pending maintenance write (source
    `pending_maintenance`; baseline = the original on-disk `content`, and the
    post-write reap verification reads `current_content` not disk, so no
    read-after-write race) and the optverify gate-flip write (source
    `optverify_resolve`).
  - `cycle_state.rs` — the per-cycle `agent_doc_pipeline:` frontmatter mirror /
    clear (sources `pipeline_mirror` / `pipeline_clear`); `try_editor_converge`
    already converges frontmatter-only deltas.
  - Deliberately left direct (audited): `maintenance.rs` queue-halt write (already
    editor-aware via `converge_live_buffer_queue_shape`), the closeout/`git.rs`
    HEAD-restore recovery paths (already editor-first / `emit_postcommit_worktree_check`),
    and `claim.rs` scaffold writes (the document is freshly scaffolded, not yet an
    open dirty buffer). New unit test
    `converge_or_disk_write_falls_back_to_plain_disk_without_listener` +
    FlowCore `reason=` budget for `write/converge.rs` bumped 8→9 (the added token
    is a test-assertion literal, no new production reason). `make check` green
    (4367 passed, 0 failed).

## Status (2026-06-12)

`#fcc0a`/`#fcc0b`/`#fcc0c`/`#fcc0d` are all committed and pushed (agent-doc
`612a8dae` + `9f0c63f8`). Every genuine direct-disk-no-IPC document-write site
(queue consume, done-id mark, `run_template`, `apply_template_from_string`) now
converges through the editor IPC when a JB listener is active; the remaining direct
writes are audited as post-IPC-timeout (#fcc0e), force_disk/no-plugin, inline (no
components), or already editor-first. `make check` green (4364 passed), clippy
clean. Binaries rebuilt (`cargo install` + `cargo build --release`) and cdylib
refreshed (`agent-doc lib-install`) — running controllers still serve the prior
build until `agent-doc admin recycle --all-projects` / session restart.

**All `#fcc0` code phases (`#fcc0a`–`#fcc0f`) are complete.** `#fcc0e` landed in full
after the operator `go` + live Phase-0 re-baseline (2026-06-12): `#stuckhandoff2`
M1–M5, the `#ipcdrift` claimed-patches/generation-fence/read-only-demotion machinery,
the `#fcc0` converger ↔ de-wedge integration (`c7054e00`), the M2 worktree-write gate
(dispatch-admission gate covers every controller-originated worktree write incl. the
autonomous idle-queue drain — now proven by
`autonomous_idle_queue_continuation_refused_when_controller_not_stable`), and the
`#pcwc` Phase-2 post-commit auto-reconcile. The M2 agent-finalize-path gate is
intentionally NOT added (redundant with the dispatch gate + M1, and gating a
legitimate finalize on the controller's state is the wrong/`#pcwc`-unsafe semantics).

**The only thing left for "0 dialogs" is the operator-drive live JB proof** (cannot be
agent-done — needs a live editor): drive a queue drain with a live IDE; confirm
`ops.log` shows `queue_consume_writeback … transport=editor_ipc` and zero disk-write
`File Cache Conflict` dialogs. Binaries + cdylib are on build `c7054e00`; the
superproject controller was recycled onto it — restart the JB/tmux session to promote
it everywhere before the proof.

## Immediate operator action

Deploy the already-committed `#w42v` compact fix so `Compact Exchange` stops
raising the dialog: `cd src/agent-doc && agent-doc lib-install` (hot-reloads the
cdylib by mtime). That removes the compact-triggered dialogs today; the phases
above remove the rest.

## Constraint

This edits the live IPC write/closeout path that a hosting session uses to
finalize its own responses — it MUST run from a dedicated CLEAN session (not a
live agent-doc IPC session), and the "0 dialogs" proof is a live JB editor check
(operator-drive). Tracked as `#fcc0`.

## Live diagnosis (2026-06-13 01:41) — why dialogs persist after the code fix

Operator re-reported dialogs. Root cause is **deployment, not code** — proven from
running-process timestamps:

- agent-doc binary + cdylib (`libagent_doc-0.34.0.so`) installed **01:20** (includes
  every `#fcc0a–f` + `#ipcdrift`/`#stuckhandoff2` de-wedge fix).
- Superproject controller (`controller serve`, PID 617249) started **01:39** — fresh,
  already recycled onto the new binary.
- **The in-process supervisor hosting `agent-doc-bugs2.md` (`start --route-owned`,
  PID 136222) started 01:00 — 20 min BEFORE the install.** It runs the OLD binary, so
  its stale-buffer / disk-fallback writes still fire the dialog. Every finalize this
  session logging `live_prompt_drift_after_preflight` → `snapshot from content_ours`
  → `postcommit_worktree_check match=false` is that stale supervisor's buffer racing
  the fresh CLI finalize.
- Supervisor self-recycle (`#ctlrecycle` R3) is **opt-in behind
  `AGENT_DOC_SUPERVISOR_AUTO_RECYCLE` (default OFF)**, so a stale supervisor silently
  persists with no surfaced signal.

**Fix for the live dialogs (operator-drive):** restart this document's agent-doc
session so its supervisor respawns on 0.34.0 — JB `Run Agent Doc` stop+restart for
the doc, or `agent-doc session restart-supervisor`, or restart the IDE/tmux session.
Cannot be agent-done from inside the session the supervisor hosts (killing PID 136222
would disrupt the live pane).

**Durable product gap (`#fccsupwarn`) — SHIPPED 2026-06-13:** preflight/session-check now
WARN when the live hosting controller/supervisor is serving a stale agent-doc binary (its
recorded `controller_binary` launch identity no longer matches `current_binary_identity`),
telling the operator to `agent-doc admin recycle` or `agent-doc session restart-supervisor`
— so this silent failure mode surfaces instead of the operator re-filing dialog reports.
Read-only detection over the controller status (NOT the live write path), fail-open. Impl:
`supervisor_stale_warning_message` / `stale_supervisor_warning_for_doc` in
`project_controller/rpc.rs` (unit-tested), wired into `preflight/run.rs` (warning code
`supervisor_binary_stale`) and `session_check.rs`. SPEC §preflight warnings updated.
Note: the restart-supervisor `--force` OS-signal escalation + external-watchdog detection
(below) remain follow-ups; this rung is the read-only operator-visible signal.

## Restart-supervisor cannot promote a pre-feature stale supervisor (2026-06-13 02:00)

Escalation finding while trying to promote PID 136222 (the stale 01:00 host):

- `agent-doc session restart-supervisor <FILE>` AND `... --force` BOTH return
  `Requested continue restart … operator_ready` and PID 136222 survives every time
  (etime passed 1h). Both only *request cooperative self-restart* via the controller;
  a supervisor predating that code never acts on it. `--force` is a no-op on exactly
  the stale supervisors that most need promotion.
- The hosting supervisor cannot be killed from inside its own session: it is the
  agent's parent (`agent-doc(136222) → claude(--continue)`, the live pane %2 agent),
  so kill/--force from within would terminate the agent issuing it.
- Every finalize / write --commit / reset --from-current is re-corrupted by the stale
  supervisor's CRDT replay (live_prompt_drift → content_ours → drifted captured
  baseline); the session document cannot be committed until the supervisor is gone.

Only escape (operator, from OUTSIDE pane %2's process tree): `kill <supervisor_pid>`
from an independent terminal, or fully restart the IDE/tmux session — the route
respawns a fresh 0.34.0 supervisor on next dispatch.

Product implications (fold into #fccsupwarn):
1. restart-supervisor --force must escalate to an OS signal (SIGTERM→SIGKILL on the
   supervisor PID) when cooperative restart is not honored within a short timeout.
2. Stale-host detection/warning must come from a process OTHER than the hosted agent
   (controller or external watchdog); the hosted agent cannot self-heal a stale parent,
   and a supervisor cannot self-recycle without killing its own live agent child.

## The shipped command that actually kills a stale supervisor (2026-06-13)

`agent-doc session interrupt-clear --force <FILE>` DOES OS-kill the supervisor PID
(`signal_pid_for_force_clear` → `libc::kill(SIGTERM)` on supervisor_pid + child_pid +
pane + socket; session_actor_cmd.rs:1276-1278). `restart-supervisor --force` does NOT
(cooperative IPC only). So the immediate unblock for a wedged stale supervisor is
`interrupt-clear --force`, not `restart-supervisor --force`.

#fccsupwarn implication #1 sharpened: `restart-supervisor --force` should reuse the
same `signal_pid_for_force_clear` supervisor-PID escalation that `interrupt-clear
--force` already has, gated on `process_binary_is_stale` (rpc.rs:940) so it only
force-kills a genuinely stale/uncooperative supervisor. Implement + test in a CLEAN
session (touches the operator-restart path; cannot be live-tested from the hosted
session it would kill).
