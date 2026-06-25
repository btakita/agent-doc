# Plan: Queue-edit-during-active-turn race + durability (consolidated) — `#qeditrace` (`#0bmv`)

**Supersedes** (both deleted; git history preserves them):
- `plan-qdurcrash-queue-edit-crash-durability.md` (`#qdurcrash`)
- `plan-supervisor-direct-queue-edit-race.md` (`#sqedit-race`)

This consolidates the two overlapping plans that both described what happens when
the operator mutates the queue while a turn / finalize / supervisor is concurrently
active: queue edits get **dropped, duplicated, mangled, or lost across a crash**.

## Unified problem

The agent-doc queue is written by **multiple concurrent actors** with no single
owner of "what the queue should be right now":

- the operator (editor buffer edits + direct `queue prune-noise` / `queue consume`),
- one or more live JB/VS Code plugin instances (`.agent-doc/patches/` watchers),
- preflight queue maintenance (`#mirrorall`, backlog→queue sync, `#7r2s` pin, dedup),
- the supervisor idle-queue-watch / route dispatch,
- the finalize queue-strike path.

When two or more of these touch the queue in the same window — especially while a
**stale editor buffer** still exists — the operator's queue edit can be undone,
re-amplified into corruption, or lost entirely (including across a supervisor/pane
crash+restart). The converge write path itself (`write/converge.rs::try_editor_converge`)
is already fail-closed; the bug class is the *compounding interaction* of the
concurrent writers plus the lack of crash-durable journaling for operator adds.

## Distinct failure modes (de-duplicated across both source plans)

1. **Multi-instance plugin re-flush (`#8bfz`).** Each live IntelliJ instance with the
   project open watches `.agent-doc/patches/` and applies/saves independently. One
   instance applies a node-strike; another re-flushes its own *stale* buffer and
   re-injects the struck lines. (`#fccreap` reaps dead-pid consumers but not live ones.)

2. **Preflight maintenance round-trips malformed entries.** When the queue already
   holds malformed entries from a prior race (double-pins `:pushpin: - :pushpin: do [#id]`,
   stray `~~~done` / runs of `---`), the parse→render round-trip re-mangles them
   (`~~~done`→`---`, a head split into `---\n---\n:pushpin: do [#id]\n---\n---`, a
   head dropped). I.e. maintenance is **not** a fixed point on malformed input.

3. **Supervisor idle-queue-watch / dispatch against a torn state.** The idle-watch
   re-reads the queue head and can re-dispatch against an intermediate, mid-edit state.

4. **Direct queue-edit undone by a concurrent writer.** A `queue prune-noise` /
   `queue consume` (or a finalize strike) made while the supervisor + live editor run
   is reverted because one of (1)/(2)/(3) re-injects what it removed — the documented
   "noise heads are never auto-deleted because the live IPC supervisor races on direct
   queue edits" hazard.

5. **Operator add lost across a crash+restart.** The operator adds a queue item, the
   turn starts, the supervisor + tmux pane crashes and restarts, and the add is GONE:
   it lived only in the editor buffer / in-memory CRDT and was never journaled to a
   crash-durable on-disk store, so restart reloaded the pre-add snapshot/HEAD.

6. **Add lost *before any cycle observes it* (pure editor-buffer).** An add that is
   killed before it is ever flushed to disk or seen by the binary cannot be journaled
   binary-side at all — it needs a plugin-side buffer-flush-on-edit.

## Root causes

- **No single authority/owner for the queue during a live window.** Multiple writers,
  no lease/arbitration → modes 1, 3, 4. (Partly closed by the leases below.)
- **Convergence is "append-then-dedup," not a per-identity lifecycle.** Every new
  duplication shape needed a new ad-hoc dedup pass; there was no invariant that "an
  item with identity X exists in exactly one lawful state" → modes 2, 4. (This is the
  exact gap `#queuestatemachine` closes.)
- **Operator queue adds were not crash-durable before ack.** The add lived only in
  volatile editor/in-memory state → modes 5, 6.
- **No epoch/owner isolation across document or supervisor-host switches.** A stale
  overlay can replay onto the wrong document → the supervisor-side amplification of
  modes 3/4. (This is what `#xdocsuper` hosting-epoch isolation closes.)

## Already-resolved by merged work (cross-reference)

The following landed on `main` and resolve large parts of the above. Verify each
remains green before treating a mode as closed.

- **`#sqedit-race` Phases 1–3 (shipped).**
  - Phase 1 (`237f5fb2`): single live plugin owner per doc — short-TTL
    `plugin_owner.rs` lease (`.agent-doc/plugin-owner/<hash>.json`, 30s); non-owner
    instances defer patch apply + `saveDocument`. **Resolves mode 1.**
  - Phase 2 (`d9a0ae58`): `queue_edit_owner` lease
    (`.agent-doc/queue-edit-owner/<hash>.json`, ~15s, RAII `QueueEditGuard`) held by
    `prune-noise` / `consume`; preflight maintenance and supervisor idle-watch defer
    while a different live process holds a fresh lease. **Resolves mode 3 and the
    preflight half of mode 4.**
  - Phase 3 (`3c4c8bbb`): idempotent malformed-entry normalization — `render(parse(x))`
    fixed point proven (`render_parse_is_fixed_point_on_malformed_queue` +
    `render_parse_fixed_point_property_over_malformed_family`). **Resolves mode 2 (the
    amplification half of mode 4).**

- **`#qdurcrash` (shipped, additive replay-only).** `queue_journal::record` durably
  appends each *observed* operator queue prompt to `.agent-doc/queue-journal/<doc-hash>.jsonl`
  (append-only + fsync) during preflight maintenance; `queue_journal::replay_missing`
  + `merge_missing_into_content` in `run_with_reap_policy` re-insert journaled prompts
  absent from the reloaded document on restart; `queue_journal::clear` prunes on
  `commit_success`. A struck/consumed prompt counts as present and is never resurrected.
  Replay-only, so it sidesteps the `#fintol2`/`#pcwc` carry-forward invariant entirely
  (never removes/rewrites content). **Resolves mode 5** (for any add observed by ≥1
  cycle before the crash).

- **`#queuestatemachine` (on main — `b20434dea` landed `#queuestatemachine2`).**
  Per-item lifecycle state machine keyed by durable head identity
  (`Authored → Mirrored → InProgress → Answered → Struck → Reaped`); convergence is
  "drive each identity to its lawful state," so a re-emit is a no-op transition and
  duplication is **structurally impossible** rather than patched. **Reinforces the
  resolution of modes 2 and 4** — it is the durable replacement for the ad-hoc dedup
  passes that Phase 3 hardened.

- **`#xdocsuper` hosting-epoch queue isolation.** `document_epoch(document_hash)`
  advances per document; a document/host switch is an epoch advance that makes a
  stale-overlay replay a no-op, with queue facts accepted with no generation gate but
  isolated per document_hash. **Reinforces the resolution of mode 3** (supervisor
  re-dispatch can no longer leak a stale queue overlay across a document/host switch).

- **crdtauth commit-barrier (`#crdtauthority`, design / partial).** The state-vector
  commit barrier flushes all currently-live connected editors to SV=N at finalize
  (a checkpoint that does not block forever on a slow/disconnected editor), replacing
  the patch-ack. This is the durable fix for the `no_ack` / `ipc_proof_insufficient`
  churn that *triggers* the degraded windows in which these races surface. It bounds
  but does not by itself eliminate the open modes below.

## Open / remaining failure modes

- **Mode 6 — pre-observation editor-buffer add lost to a crash (OPEN, plugin-side).**
  An add killed before any cycle observes it cannot be journaled binary-side. Needs a
  JB/VS Code plugin **buffer-flush-on-edit**: write the operator's queue edit to the
  on-disk document (or a sidecar) immediately, so the binary observes it on the next
  cycle and a restart can replay it via the existing `#qdurcrash` journal. Operator-
  install / live-verify gated.

- **Operator live-verification of mode-4 closeout (`#sqedit-race` Phase 4, OPEN,
  operator-gated).** After the shipped phases: `make install` → `agent-doc admin
  recycle` (fresh editor buffer, no stale re-flush) → `agent-doc queue prune-noise
  tasks/agent-doc/agent-doc-bugs2.md`; confirm `session-check` `queue_stale_noise_lines`
  drops to 0 and sticks across a subsequent preflight (no re-injection). Folds in
  `#prunenoise-live` and `#qpausemix-verify`. Cannot be agent-completed — needs a live
  editor + supervisor recycle.

- **End-to-end multi-writer + crash convergence proof (OPEN, hardening).** A single
  SimWorld scenario that combines a two-writer queue race, a `kill -9` of the
  supervisor mid-turn, and the state-machine convergence — asserting one item per
  identity survives, no stale re-injection, and operator position-lock is preserved.
  The constituent proofs exist in the individual plans; the unified scenario does not.

## Phased fix approach (remaining work)

The bulk of the original two plans is **already shipped on main** (see above). What
remains is a small, well-scoped tail:

- **Phase A — plugin buffer-flush-on-edit (closes mode 6).** Plugin writes the
  operator's queue edit to disk/sidecar immediately so the binary can observe + journal
  it. Reuses the existing `#qdurcrash` journal/replay; no binary CRDT/reconcile change.
  Operator-install + live-verify gated.

- **Phase B — operator live closeout (`#sqedit-race` Phase 4).** The recycle +
  `prune-noise` live verification above. Operator-driven.

- **Phase C — unified convergence proof (hardening).** The combined SimWorld scenario
  (two-writer race × crash × state-machine convergence). Agent-doable in a focused
  clean cycle with `make check`; the supervisor-touching parts need `make tmux-ci`
  across two live panes. Do **not** land from a degraded drain.

## Acceptance

- A `queue prune-noise` / `consume` against a live supervisor + editor is not
  re-injected by a second plugin instance, preflight maintenance, or the idle-watch.
- A preflight on an already-malformed queue converges to a clean fixed point.
- An operator queue add observed by ≥1 cycle survives a `kill -9` + restart.
- An operator queue add killed pre-observation survives once Phase A ships.
- SimWorld coverage for the two-writer race, the render/parse fixed point, and the
  unified crash×convergence scenario.
- `make check` (+ `make tmux-ci` for supervisor/editor-touching changes) green.

## Related

`#8bfz` (single-owner election), `#cyh0`/`#fcczero` (FCC zero disk writes),
`#aqhr`/`#smconv`/`#semmerge` (converge semantic merge, 2026-06-18), `#prunenoise-live`,
`#qpausemix-verify`, `#fintol2`/`#pcwc` (carry-forward invariant — the reconcile risk
gate that the replay-only `#qdurcrash` design deliberately avoids), `#queuestatemachine`
(per-item lifecycle SM, on main), `#xdocsuper` (hosting-epoch isolation),
`#crdtauthority` (commit barrier), `#qdispatchloss` (uncommitted-head dispatch defer,
shipped 2026-06-19), `#routedeadshell`/`#1vhn` (the crash event class behind mode 5).
