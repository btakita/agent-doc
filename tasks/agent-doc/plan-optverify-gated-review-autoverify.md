# Plan: opportunistic gated-review auto-verification (`#optverify`)

Goal: let agent-doc **auto-prove or auto-disprove** gated `[/]` review items from
`ops.log` markers during normal `preflight`/closeout — no dedicated live-verify
session. Markers fire during *any* session activity, so a gated item verifies
opportunistically as the tool is used. Implements the operator model: "no
live-verify gate / adequate logging / I file bugs, you check the logs."

Spec home: `specs/07-closeout-commands.md` (gated-review proof predicates).

## Why

Gated `[/]` review items today need a human to drive a live action and an agent
to eyeball `ops.log`. Most gates already name an exact `ops.log` marker that
proves them (e.g. `live_prompt_drift_auto_recovered`) and an exact string that
disproves them (e.g. `looks like a manual cleanup`). If the gate carries those as
typed predicates, the binary can decide proof/disproof itself every time it
already reads `ops.log` — turning a standing human gate into an automatic check.

## Already shipped (do not redo)

- `--pending-set-gate-type <id=gate_type>` exists (sets a gate-type **string** on
  a gated item). This plan extends the gate-type into a structured proof/disproof
  predicate; it does **not** add a new flag family from scratch.
- `preflight` already reads `ops.log` / session-log and reports `review_count` /
  `review_gated_count`. The scan hooks into that existing read, not a new pass.
- ops.log proof-diagnostic plumbing (`ops_log`, `ipc_proof_insufficient`,
  `flow_event`) already exists for emitting structured closeout markers.

## Phases (one backlog id per actionable phase)

- **`#optv1` — typed proof/disproof predicate on the gate.** Extend the gated-item
  gate-type so it can carry `verify=ops_log:<marker>` (proof) and
  `disproof=ops_log:<text>` (disproof), plus the **gate-set timestamp** (so the
  scan only considers markers emitted *after* the gate was set — confirm whether
  the `[/]` item already records a set time; add one if not). Parse + serialize +
  round-trip on the existing `--pending-set-gate-type` path. Pure data-model unit
  tests. No scanning yet.
- **`#optv2` — ops.log marker scan (pure).** Pure function over (gated item
  predicate, gate-set timestamp, ops.log content) → `{Provable(marker, at) |
  Failed(marker, at) | Pending}`. Proof marker seen after the gate time + disproof
  absent → `Provable`; disproof seen → `Failed` (disproof wins ties, fail safe);
  neither → `Pending`. Unit-test against committed fixture ops.log slices.
- **`#optv3` — preflight/closeout surfacing + transition.** Wire `#optv2` into the
  existing preflight ops.log read: for each gated item with a predicate, surface
  `review #X provable (marker Y @ t)` and offer `[/]→[x]` (auto-resolve only under
  an explicit opt-in, default off — never silently flip a human gate); surface
  `review #X FAILED: <marker>` for the operator to file a bug. Emit a structured
  `optverify` diagnostic to `ops.log`. Keep stdout quiet unless verbose
  (`#opencode-stdout-bleed`).
- **`#optv4` — cleanup.** Dedupe the stale duplicate `#bkx9perid` backlog entry
  noted in the original item (now archived as `#bkx9wire`/`#bkx9perid` in
  `tasks/agent-doc-bugs2.done.md`); ensure no stale duplicate lingers.

## Safety invariants

- **Never silently flip a human gate.** `[/]→[x]` auto-transition is opt-in
  (default off); without the opt-in the binary only *surfaces* "provable" and
  leaves the operator to confirm.
- **Disproof wins.** If both proof and disproof markers appear, treat as `Failed`
  (a disproved gate must never auto-resolve).
- **Time-scoped.** Only markers emitted after the gate-set timestamp count, so a
  stale pre-gate marker cannot falsely prove a freshly re-opened gate.
- **No predicate → unchanged.** Gated items without a typed predicate behave
  exactly as today (manual gates).

## Verification obligations

Pure unit coverage for `#optv1`/`#optv2` (predicate round-trip, scan decision
matrix incl. disproof-wins + time-scoping). Integration coverage for `#optv3`
surfacing and the opt-in transition. `#optv4` is a doc/backlog cleanup.

## Run as a fresh focused session

Multi-phase feature touching the pending/gate data model, preflight, and spec.
Implement `#optv1`→`#optv3` with unit coverage in a clean session; `#optv4` is a
trivial cleanup. Not suited to an end-of-drain degraded session.
