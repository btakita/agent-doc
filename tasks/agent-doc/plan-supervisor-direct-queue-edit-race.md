# Plan: Fix the live-IPC-supervisor race on direct queue edits (#sqedit-race)

## Problem

A direct queue edit (`agent-doc queue prune-noise` / `queue consume`, or a finalize
queue strike) made while the route-owned supervisor + live JB editor are running can
be undone or amplified into corruption. This is the documented "noise heads are
**never auto-deleted** (the live IPC supervisor races on direct queue edits)"
hazard, and the reason `#prunenoise-live` / `#qpausemix-verify` are gated on a
supervisor recycle.

It is **not** a single bug in the converge write path — `write/converge.rs::try_editor_converge`
is already fail-closed (refuses on `ack_mismatch` / `no_ack`, never lets a stale
editor buffer silently win). The race is the compounding interaction of three
concurrent queue writers plus a stale editor buffer:

1. **Multiple live plugin consumers (#8bfz).** Each IntelliJ instance with the
   project open watches `.agent-doc/patches/` and applies/saves independently
   (`jetbrains-<pid>-<uuid>`). One applies the node-strike; another re-flushes its
   own *stale* buffer and re-injects the struck lines. `#fccreap` reaps dead-pid
   consumers but does not dedupe LIVE ones.
2. **Preflight queue maintenance round-trips malformed entries.** Every preflight
   runs `#mirrorall` (re-mirror operator-verify heads), backlog→queue sync,
   `#7r2s` auto-pin, and live-editor-buffer convergence. When the queue already
   holds malformed entries from a prior race (double-pins like
   `:pushpin: - :pushpin: do [#id]`, stray `~~~done` / runs of `---`), the
   parse→render round-trip re-mangles them (observed this session: `~~~done`→`---`,
   one head split into `---\n---\n:pushpin: do [#id]\n---\n---`, `#fbwire` head
   dropped).
3. **Supervisor idle-queue-watch** re-reads the queue head and can re-dispatch
   against a torn intermediate state.

## Fix (phased — SAFETY-CRITICAL supervisor/commit-path; focused clean cycle only)

Each phase needs `make check`; phases touching the supervisor/editor need
`make tmux-ci` across two live panes. Do **not** land from a degraded drain.

- **Phase 1 — single live plugin owner per doc (#8bfz, foundational). ✅ SHIPPED**
  (commit `237f5fb2`). Plugin acquires a short-TTL per-document owner lease
  (`plugin_owner.rs` → `.agent-doc/plugin-owner/<hash>.json`, 30s TTL); non-owner
  live instances defer patch application + `saveDocument` (`PatchWatcher.ownsDocument()`
  gate, release on dispose). Binary lease primitive + FFI
  (`agent_doc_plugin_owner_try_acquire`/`_release`) + 5 unit tests. Removes
  re-flush source (1).

- **Phase 2 — queue-edit lease respected by all writers. ✅ SHIPPED** (commit
  `d9a0ae58`). Direct queue-edit commands (`prune-noise`, `consume`) hold a new
  sibling `queue_edit_owner` lease (`.agent-doc/queue-edit-owner/<hash>.json`, ~15s
  TTL, RAII `QueueEditGuard`) for the edit's duration. Both other concurrent
  writers now defer while a different live process holds a fresh lease:
  **preflight queue maintenance** (`run_queue_maintenance` early-returns without
  mutating — covers `#mirrorall` mirror, backlog→queue sync, `#7r2s` pin, dedup)
  and the **supervisor idle-queue-watch** (skips the dispatch tick). Removes source
  (3) and the preflight half of (2). 5 lease unit tests + maintenance defer
  integration test; full suite (4796) + clippy green. Install/recycle deferred to
  Phase 4.

- **Phase 3 — idempotent malformed-entry normalization. ~ MOSTLY LANDED**
  (incremental: `#qdup-bare-id` collapses duplicate bare `[#id]` mirror heads,
  `#qnoise-multiline-strike` excises fenced/multiline pasted-evidence heads,
  `#pushpinaccum` idempotent operator pin). Remaining: a property/SimWorld
  `render(parse(x))` fixed-point test over an already-malformed queue (double-pins
  + stray `~~~done`/`---` runs together) to prove convergence, not amplification.
  Make the queue
  parse/render path collapse double-pins, strip stray `~~~done` / consecutive `---`
  separator runs, and de-duplicate identical adjacent heads **idempotently**, routed
  through the converge path (never a bare disk write under a live listener). A
  round-trip on an already-malformed queue must converge, not amplify. Add
  property/SimWorld coverage that render(parse(x)) is a fixed point. Removes the
  amplification half of (2).

- **Phase 4 — restart + noise cleanup (the operator-gated finish).** After Phases
  1–3 ship: `make install` → `agent-doc admin recycle` (fresh editor buffer, no
  stale re-flush) → `agent-doc queue prune-noise tasks/agent-doc/agent-doc-bugs2.md`.
  Confirm `session-check` `queue_stale_noise_lines` drops to 0 and the prune sticks
  across a subsequent preflight (no re-injection). Folds in `#prunenoise-live` and
  the `#qpausemix-verify` live check.

## Acceptance

- A `queue prune-noise` / `consume` run against a live supervisor + editor is not
  re-injected by a second plugin instance, preflight maintenance, or the idle-watch.
- A preflight on an already-malformed queue converges to a clean fixed point instead
  of re-mangling entries.
- SimWorld coverage for the two-writer race and the render/parse fixed point.
- `make check` + `make tmux-ci` green.

## Related

`#8bfz` (single-owner election — becomes Phase 1), `#cyh0`/`#fcczero` (FCC zero
disk writes), `#aqhr`/`#smconv`/`#semmerge` (converge semantic merge — landed
2026-06-18), `#prunenoise-live`, `#qpausemix-verify`.
