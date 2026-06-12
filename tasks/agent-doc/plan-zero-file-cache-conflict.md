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
- **#fcc0e** ◑ MOSTLY DONE — Resolve the `#ipcdrift`/`#stuckhandoff2` wedge so IPC
  stops failing. Three sub-surfaces; the agent-completable ones have landed:
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
  - **REMAINING (operator-gated):** M2's full finalize/commit **worktree-write**
    gate on `Stable` — overlaps the contested `#pcwc` carry-forward invariant
    (memory: don't re-wire without operator go + live Phase-0 re-baseline). M1
    already bounds the Preparing window, so the wedge can't persist long enough to
    corrupt much before self-reap. This is the only remaining code lever, and it is
    the user's call.

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

**#fcc0e** is now mostly landed (`c7054e00` + the prior `#stuckhandoff2` M1-M5 and
`#ipcdrift` claimed-patches/generation-fence/read-only-demotion machinery). The
only remaining code lever is **operator-gated**: M2's full finalize/commit
worktree-write gate on `Stable`, which overlaps the contested `#pcwc` carry-forward
invariant and must not be re-wired without operator go + a live Phase-0 re-baseline.

Remaining for "0 dialogs": the operator decision on the M2 worktree-write gate, and
the operator-drive live JB proof (drive a queue drain with a live IDE; confirm
`ops.log` shows `queue_consume_writeback … transport=editor_ipc` and zero disk-write
dialogs). Binaries + cdylib are rebuilt; recycle controllers / restart sessions to
promote build `c7054e00`.

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
