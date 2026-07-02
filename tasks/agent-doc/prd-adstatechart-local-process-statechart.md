# PRD: Local-process Harel state chart for transport / editor-sync / closeout (`#adstatechart`)

**Phase E** of the 5-root-cause IPC audit (`A → D → B → C → E`). E is the
consolidation rung: once A–D encode the *correct* guards, this replaces the
scattered booleans/string-enums that encode those guards implicitly with one
named, reactive Harel chart per process.

## Status (2026-07-02)

- **Dependency (Phase 0) — DONE.** `lazily` 0.18.0 published (statechart API).
  Then `lazily` **0.19.0** published (crates.io + tag `v0.19.0`, commit
  `458d914` on lazily-hub/lazily-rs) adding: a typed Rust `ChartBuilder`
  (definition in Rust *or* JSON, both via the shared `from_states`), and
  `ThreadSafeStateChart` + `AsyncStateChart` context variants (the flat
  `StateMachine`'s three-context parity). Engine extracted context-free;
  conformance fixtures 7/7 + 104 lib tests green.
- **Rung 1 (`#adstatechart1`) — DONE.** `agent-doc-state-backbone::adstatechart`
  (commit `8000c8cf`): `ChartFacts` + pure guards + the four-region ChartDef +
  the load-bearing rejected-edge proof (`commit` while editor-ahead). 8 tests
  green. On `lazily` 0.18 single-threaded `StateChart`, JSON def. Not installed
  (decomposition deploy gate).
- **Cascade (follow-ups, queued):** switch rung 1 to the typed builder + bump
  agent-doc to `lazily` 0.19 + use `ThreadSafeStateChart` for the live wiring;
  rungs 2–4 below; lazily-kt (Maven) + lazily-js (npm) StateChart + threading +
  builder for the editor plugins; lazily-spec prose note (Pages) that StateChart
  has all three context variants; a focused **Lean model** proving the rung-1
  invariants (rejected-edge safety on all paths + no dead-end config), scoped to
  per-process safety only — it cannot prove production wedge-freedom (real wedges
  are "the unblocking event never fires", a distributed-liveness property outside
  the per-process chart, same boundary as root cause B).

## Operator ask

> lazily-rs now supports State Charts. Would a State Chart improve state
> management, observability, and locatability? … do phase E

Answered yes in the exchange, **scoped to per-process local state**. This PRD
fixes that scope in writing so implementation cannot drift into trying to model
distributed failures with a per-process chart.

## Why (root cause E)

Almost every bug in `agent-doc-bugs2.md` is an *implicit* state machine encoded
as scattered booleans and string enums, so impossible states are both
representable and reachable. A forgotten guard at one of N call sites is the
recurring failure shape (`#boundaryaccum`, `#live_prompt_drift`,
`#qftlossdelta`). The chart makes the illegal transition a **rejected edge**
instead of an ad-hoc guard re-checked (and eventually forgotten) at each site.

## Scope boundary (load-bearing — do not cross)

**In scope — per-process local state only:**
- IPC transport region: `socket → degraded → file_fallback`
- editor-sync region: `synced → editor_ahead(stale_disk) → publishing`
- closeout region: `idle → written → committed → session_check_ok`
- supervisor region: `busy / idle / stale_binary / recycle_yield`

**Explicitly OUT of scope (stays git/CRDT authority + recycle, NOT a chart):**
- stale-**binary** supervisor vs newer finalize client (`stale_install`)
- concurrent sessions force-pushing the shared superproject
- editor-plugin vs CLI version skew (that is root cause **B** `#ipcverhandshake`)

The chart is **compute, not a wire protocol**, and per-process. It orchestrates
transitions; it is not a distributed-consensus engine. A per-process chart would
NOT have prevented this session's stale-cdylib-with-no-live-supervisor wedge.

## lazily-rs statechart API constraints (source-of-truth: `src/lazily-rs/src/statechart.rs`)

- `ChartDef::from_json(&Value)` — declarative Harel chart: compound + orthogonal
  (parallel) regions, shallow/deep history, entry/exit/transition actions, named
  guards, internal/external transitions.
- `StateChart::new(ctx, def)` → reactive; the active configuration is a
  `CellHandle<BTreeSet<String>>`. `configuration()`, `active_leaves()`,
  `matches(id)` are cell reads (invalidated only on a real transition; a no-op
  self-transition is suppressed by the cell `PartialEq`).
- `send(ctx, event, guards: &HashMap<String,bool>) -> bool`.
- **Rejects `{"expr": …}` extended-state guards and `run` actions.** So a
  comparison like `edit_epoch > last_synced_epoch` MUST be computed in Rust and
  passed in as a **named boolean guard** (`editor_synced`) to `send`. Guards
  stay pure and unit-testable; the chart never reads extended state.
- Requires the `statechart` cargo feature.

## Chart definition (orthogonal regions under one root)

```
root (parallel)
├─ transport
│   ├─ socket          (initial)
│   ├─ degraded        ── on send_timeout / no_ack
│   └─ file_fallback   ── on no_ipc_listener | recovery=direct_write_fallback
├─ editor_sync
│   ├─ synced          (initial)
│   ├─ editor_ahead    ── guard: !editor_synced (edit_epoch > last_synced_epoch)
│   └─ publishing      ── publish_live_buffer in flight (socket or file-signal)
├─ closeout
│   ├─ idle            (initial)
│   ├─ written         ── write ok
│   ├─ committed       ── commit ok  [guard: editor_synced && !editor_ahead]
│   └─ session_ok      ── session-check ok (final for the cycle)
└─ supervisor
    ├─ idle            (initial)
    ├─ busy
    ├─ stale_binary    ── observed build older than installed
    └─ recycle_yield   ── recycle-yield requested
```

The **illegal `finalize` while editor epoch is ahead** becomes a rejected edge:
the `closeout: written → committed` transition carries guard
`editor_synced && !editor_ahead`; `send("commit", {editor_synced:false})`
returns `false` instead of committing from a stale disk image. That is the
`#live_prompt_drift` / "refusing to commit from stale disk" guard, encoded once.

## Wins this delivers

- **State management** — impossible states unrepresentable; the forgotten-guard
  class collapses to one edge definition.
- **Observability** — one named `configuration()` snapshot
  (`transport.file_fallback × editor_sync.editor_ahead × closeout.written`)
  directly loggable by `session-check`/status, instead of grepping `ops.log` for
  epoch numbers and reconstructing state. Reactive: status re-renders only on a
  real transition (self-transition suppressed) — the "don't re-print unchanged
  status" behavior the loop already wants.
- **Locatability** — `#live_prompt_drift` becomes "transition `commit` fired in
  region `closeout` while guard `editor_synced` was false": one edge, not a
  call-site hunt. Maps onto the tag-per-bug culture.

## Relationship to existing FSM

Not greenfield. `#mergestatemachine3` / `#lazilystatesync`
(`tasks/agent-doc/plan-merge-control-state-machine.md`) already routes
JB+VSCode attach/detach/ownership through a **flat** merge-ownership FSM. This
Harel chart is the natural upgrade: keep the merge-ownership region as one
region, add the orthogonal transport / editor-sync / closeout / supervisor
regions. Do NOT duplicate the merge-ownership logic — compose it in.

## Sequencing gate

Phase E consolidates **after A–D encode the correct guards**. Current backlog:
A `#ipcauthority` (down-payment shipped), D `#ipcliveness` (partial), B
`#ipcverhandshake`, C `#ipcpostcommitinv`. The chart should encode the guards
those items settle on, not pre-empt them. **This PRD does not require A–D to be
fully done to land the chart skeleton (`#adstatechart1`), but the guard
semantics (`#adstatechart2`) must follow the guards A–D actually ship.**

**Deploy gate:** `src/agent-doc` currently carries the uncommitted
module-decomposition refactor (122 files, `#coredecomp`/`#orchdecomp`). Do NOT
`make install`/push chart work until the decomposition lands (same gate as
`#staleplugindeploy`) — installing would deploy the decomposition WIP as the
live binary. Chart source/tests can be authored + `make check`ed against the
decomposition tree, but not installed.

## Phases

1. **`#adstatechart1`** — author the `ChartDef` JSON (four orthogonal regions
   above) behind the `statechart` feature; pure Rust guard computations
   (`editor_synced`, `transport_degraded`, `supervisor_stale`) with exhaustive
   unit tests; NO integration into the live write path yet. Prove the illegal
   `commit`-while-`editor_ahead` edge is rejected. Deliverable: chart + guard
   module + tests, `make check` green (not installed).
2. **`#adstatechart2`** — wire `session-check`/status to *read* `configuration()`
   for observability only (advisory, non-load-bearing): log the named config
   snapshot alongside today's markers. No behavior change to closeout. Guard
   semantics must match whatever A–D shipped.
3. **`#adstatechart3`** — make the chart load-bearing for the closeout guard:
   replace the ad-hoc `editor_synced` check at the finalize/commit site with
   `send("commit", guards)`; keep a fail-closed fallback if the feature is off.
   SimWorld coverage for N wedged-finalize→direct-commit cycles proving the
   rejected edge holds. Live editor proof gated behind decomposition + install.
4. **`#adstatechart4`** *(optional/future)* — fold the `#mergestatemachine3`
   flat merge-ownership FSM into the chart as a fifth orthogonal region so
   attach/detach/ownership + transport + closeout share one `configuration()`.

## Testing

- Pure guard fns: exhaustive unit tests (each named guard true/false).
- Chart transitions: table-driven `send` tests per region, incl. the rejected
  `commit`-while-`editor_ahead` edge and no-op self-transition suppression.
- SimWorld: N wedged-finalize→direct-commit cycles never commit from stale disk.
- `make check` green against the decomposition tree; live editor proof deferred
  behind the deploy gate.
