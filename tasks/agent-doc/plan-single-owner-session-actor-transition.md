# Plan — transition `agent-doc` to a single-owner session actor model

**Pending:** `#sgown`
**Status:** Draft

## Problem

`agent-doc` is still letting multiple subsystems make ownership decisions for the same document:

- supervisor/session log
- tmux registry and pane/window topology
- route/sync/resync recovery paths
- editor plugin dispatch and layout reporting

That creates exactly the drift the recent `agent-doc-bugs2.md` failures keep exposing. A routed editor action can land while one source still points at the old pane, another source has already rebound to a successor pane, and a third source is still waiting for prompt readiness or document-cycle acknowledgement. Each extra heuristic reduces one symptom but increases the overall race surface.

The architectural fix is to stop reconciling competing ownership guesses on the normal path.

## Target Model

Make one per-document session actor the only authority for ownership.

Authoritative state:

- `document_id`
- `session_id`
- `generation`
- `pane_id`
- `window_id`
- `harness`
- `state` (`starting`, `ready`, `busy`, `waiting_input`, `closed`, `blocked`)
- `last_transition`

Rules:

1. Only the actor mutates authoritative ownership.
2. Every start, reroute, restart, attach, and handoff creates a new generation.
3. Route/sync/plugins never infer ownership from process trees, tmux topology, or stale session-log history on the normal path.
4. Registry, session logs, and editor-visible state become projections and diagnostics, not sources of truth.
5. Ambiguous or stale state fails closed instead of auto-repairing through hidden tmux mutation.

## Core Invariants

### Ownership

- Exactly one active authoritative owner exists per tracked document generation.
- A pane can only become authoritative through an actor transition, never through passive discovery.
- A stale generation can never reclaim ownership after a newer generation is committed.

### Tmux safety

- Normal turn commands do not kill panes and do not create duplicates.
- Pane-destroying or pane-replacing actions live behind explicit repair commands only.
- If a pane already owns the document, `start`/`claim`/`route` fail closed with concrete inspection and cleanup commands instead of auto-superseding it.

### Dispatch

- Dispatch success is proven by an actor transition such as `generation_started` or `generation_ready`, not by later document diff observation alone.
- Editor actions send commands to the actor; they do not inject raw reopen text into a pane unless the actor has already admitted that generation.

### Projection

- `sessions.json`, tmux layout state, prompt polling state, and plugin UI state are derived from the actor record.
- Session logs remain append-only audit/debug artifacts.

## Phased Transition

### Phase 1: Freeze the contract

- Write a focused spec for the actor-owned session model and generation semantics.
- Mark current heuristic ownership proofs as legacy/fallback only.
- Add structured logging for every ownership transition:
  - prior generation
  - new generation
  - old pane/window
  - new pane/window
  - caller (`start`, `route`, `sync`, plugin action, repair)
  - reason

### Phase 2: Introduce the actor store

- Add a durable actor record store keyed by canonical document path.
- Start with a simple local store that supports atomic compare-and-swap on generation.
- Keep `sessions.json` as a read projection during migration.
- Evaluate sqlite only if the simpler store cannot provide the atomicity and inspection needed; do not introduce it just because it is available.

### Phase 3: Move supervisor ownership into the actor

- Make supervisor startup register through the actor instead of writing ownership directly into registry/session history.
- Add explicit actor commands for:
  - `start`
  - `attach`
  - `dispatch`
  - `restart`
  - `mark-ready`
  - `mark-busy`
  - `mark-waiting-input`
  - `close`
- Have the supervisor report prompt-readiness and closeout events as actor transitions.

### Phase 4: Convert route to actor dispatch

- Replace normal-path route ownership recovery with:
  - read actor record
  - validate generation/state
  - send dispatch request
  - wait for bounded actor acknowledgement
  - fail closed on timeout or stale generation
- Remove route-side direct re-registration, opportunistic same-file pane stealing, and cross-source owner election from the normal path.

### Phase 5: Convert sync to projection reconciliation only

- `sync --no-autostart` should stop trying to prove or repair ownership from tmux/process evidence.
- Sync should ask the actor which pane generation is authoritative, then reconcile layout around that answer.
- If a visible file has no authoritative owner, sync may request a fresh start through the actor, but only as an explicit new generation.
- Mixed-root visible layouts remain fail-safe: unresolved files preserve the current visible layout instead of forcing replacement.

### Phase 6: Shrink repair into explicit commands

- Move destructive or heuristic recovery into clearly named commands such as `repair`, `fix`, or `doctor --repair`.
- Keep those paths opt-in and heavily instrumented.
- Remove hidden pane-kill, stash replacement, and silent duplicate-suppression branches from normal turn commands.

### Phase 7: Delete legacy ownership inference

- Remove normal-path process-tree ownership proof.
- Remove normal-path latest-session-log owner inference.
- Remove registry-rebind-as-authority logic from route/sync.
- Keep only targeted diagnostics that explain why a stale pane exists, not logic that lets it win authority again.

## Execution Ownership Boundaries

The remaining phases should not all be treated the same from an execution model
standpoint. Some work can be parallelized into bounded sidecars after the
authoritative contract is fixed for that phase; other work must remain
single-owner because it mutates the same ownership boundary that this plan is
trying to collapse.

### Hard rule

- Any change that can alter authoritative ownership, generation monotonicity,
  actor state transitions, or normal-path dispatch/sync decisions stays
  single-owner.
- Sidecars are only allowed for disjoint, bounded follow-up work such as
  read-only investigation, test-only additions, editor-specific adapters, or
  docs once the owner-facing contract for that phase is already frozen.
- Sidecars must never independently edit `start`, `route`, `sync`, `sessions`,
  or actor-store ownership logic in parallel with another active owner change.

### Phase-by-phase allowance

| Phase | Execution model | Why |
| --- | --- | --- |
| `#sgown5` sync/focus projection reconciliation | Single-owner for actor/sync semantics; bounded sidecars allowed only for read-only reproduction or isolated regression coverage after the projection contract is fixed | Sync/layout reconciliation still decides how visible panes map to the authoritative generation; concurrent ownership edits would reintroduce the race surface |
| `#sgown6` operator CLI + plugin controls | Mixed: single-owner for actor command semantics (`status`, `attach`, `restart`, `clear`, `doctor`), bounded sidecars allowed for disjoint JetBrains / VS Code wiring once the binary contract is fixed | The actor command contract is shared state; plugin menu wiring and UI affordances can be split only after the command surface is stable |
| `#sgown7` repair-only isolation | Single-owner | This phase moves destructive behavior behind explicit repair gates, so parallel edits across recovery paths would blur the exact boundary the phase is defining |
| `#sgown8` delete legacy ownership inference | Single-owner | Removing the last heuristic owner-election branches must be sequenced through one authority map; splitting that work risks half-deleted fallback logic and stale authority leaks |
| `#sgown9` regression + plugin verification | Bounded sidecars encouraged when write scopes are disjoint | Once the ownership semantics above are landed, verification can fan out across tmux regressions, plugin coverage, and diagnostics checks without mutating the authoritative path |

### Operational guidance

- Treat phases `#sgown5`, `#sgown7`, and `#sgown8` as one-owner execution only.
- Treat phase `#sgown6` as "binary contract first, plugin adapters second";
  sidecars may own only the adapter/UI layer after the shared command contract
  is merged.
- Treat phase `#sgown9` as the main place to use bounded sidecars, because the
  work is naturally separable into disjoint verification slices.
- If a task touches both ownership semantics and verification/UI in one prompt,
  land the ownership change first and queue the sidecar-eligible slice as a
  follow-up step instead of running both in parallel.

## Developer Ergonomics

Operators need direct visibility into actor state or they will not trust the model.

### New CLI surfaces

- `agent-doc session status <FILE>`
  - prints authoritative generation, pane, window, harness, state, and last transition
- `agent-doc session history <FILE>`
  - prints the actor transition log for that document
- `agent-doc session attach <FILE> --pane %123`
  - explicit manual ownership handoff, creating a new generation
- `agent-doc session restart <FILE> [--fresh|--continue]`
  - actor-owned restart instead of route-side restart heuristics
- `agent-doc session clear <FILE>`
  - actor-owned `/clear` equivalent for the bound harness session
- `agent-doc session doctor <FILE>`
  - read-only diagnostic summary; add `--repair` only on explicit destructive follow-up

### Plugin UI actions

- Rename the current mental model from "send text into tmux" to "dispatch to session".
- Keep `Run Agent Doc`, but make its success/error UI actor-state-based.
- Keep `Claim for Tmux Pane`, but reframe it as an explicit attach/handoff action.
- Add `Show Session Status`.
- Add `Restart Session`.
- Add `Clear Session Context`.
- Add `Copy Session Diagnostics`.

### Plugin feedback expectations

- Immediate inline hint when a dispatch request is accepted by the actor.
- Persistent error with exact stage when it fails:
  - no authoritative owner
  - owner busy
  - generation stale
  - pane missing
  - waiting on user input
- Optional status panel or popup section showing:
  - file
  - generation
  - pane
  - harness state
  - last transition time

## Command Mapping During Migration

Current editor commands and their intended target model:

- `Run Agent Doc`
  - today: `agent-doc route --dispatch-only <file>`
  - target: `agent-doc session dispatch <file>`
- `Claim for Tmux Pane`
  - today: `agent-doc claim <file> --position ...`
  - target: `agent-doc session attach <file> --pane ...`
- `Sync Tmux Layout`
  - today: `agent-doc sync --no-autostart ...`
  - target: projection reconcile around actor-owned panes, with optional explicit start requests
- `Tab-to-Pane Sync`
  - today: `focus` / passive `sync --no-autostart`
  - target: focus/layout projection only, never ownership inference
- `Clear Session Context`
  - target for both JB and VS Code contextual menus: actor-owned harness clear command routed to the authoritative generation

## Verification Plan

- Add unit coverage for generation monotonicity and stale-generation rejection.
- Add route/sync integration tests proving a stale pane cannot reclaim ownership after a newer generation starts.
- Add tmux-backed tests for:
  - explicit attach handoff
  - dispatch to ready owner
  - busy owner fail-closed behavior
  - restart producing a new generation
  - sync preserving layout when one visible file is unresolved
- Add plugin tests covering:
  - session-status display
  - clear-context action wiring
  - persistent stage-specific dispatch errors

## Likely Files

- `src/agent-doc/src/start.rs`
- `src/agent-doc/src/route.rs`
- `src/agent-doc/src/sync.rs`
- `src/agent-doc/src/resync.rs`
- `src/agent-doc/src/sessions.rs`
- `src/agent-doc/src/supervisor/*`
- `src/agent-doc/src/ffi.rs`
- `src/agent-doc/specs/supervisor.md`
- `src/agent-doc/specs/07-session-tmux-commands.md`
- `src/agent-doc/specs/08-session-routing.md`
- `src/agent-doc/editors/PLUGIN-SPEC.md`
- `src/agent-doc/editors/SPEC.md`
- `src/agent-doc/editors/jetbrains/*`
- `src/agent-doc/editors/vscode/*`

## Exit Criteria

- Normal `start`, `route`, `sync`, `preflight`, and `finalize` paths no longer infer ownership from multiple mutable subsystems.
- One actor record is the only authoritative owner for each document generation.
- Plugin and CLI operators can inspect, clear, restart, and explicitly hand off a session without reading tmux internals or raw logs.
- Ambiguous situations fail closed with actionable commands instead of silently mutating tmux state.
