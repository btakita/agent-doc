# Plan — Supervisor picks up a mid-session `agent:` change (#agentchange / #5)

## Operator request

> "When I change the agent, the supervisor state should pick up the change and be
> able to dispatch to the new agent."

When the operator edits the frontmatter `agent:` field mid-session (e.g.
`claude` → `codex`), the running route-owned supervisor must dispatch the NEXT
turn to the NEW agent instead of continuing to launch the agent it cached at
startup.

## Phase 1 — Investigation (current behavior, with citations)

**Finding: this is ALREADY IMPLEMENTED end-to-end on this branch's HEAD.** The
feature shipped as `#agentreloadrestart` Phase 1a + 1b in commits:

- `8ce4110f` — Phase 1a: harness-change detection + boundary-gate logging
- `603ce7a2` — Phase 1b: re-resolve harness + fresh-spawn on the agent change

Both are ancestors of this worktree HEAD (`27bf61a6`).

### Where/when the agent is resolved

- At supervisor startup, the harness is resolved once from frontmatter:
  `agent-doc-orchestration/src/start/run.rs:289`
  (`HarnessConfig::from_context(&fm, &global_config)`), and the launched binary
  is captured for the watch loop as `launch_harness_binary`
  (`idle_watch.rs:555`).
- `HarnessConfig::from_context` resolves frontmatter `agent` > config
  `default_agent` > `claude` (`harness.rs:118`). Single source of truth.

### Re-resolution at the turn/cycle boundary (NOT cached-only)

The supervisor does NOT stay pinned to the startup value. Two cooperating pieces:

1. **Detection + boundary gate (idle watch).**
   `idle_watch.rs:723-804` re-reads the CURRENT frontmatter each idle tick,
   re-resolves via `from_context`, and compares the resolved binary to
   `launch_harness_binary` (`idle_watch.rs:728-729`). On a change it runs the
   pure boundary policy `agent_change_restart_decision`
   (`decisions.rs:663-676`): only `Restart` when the harness changed AND the
   knob is on AND the pane is at a quiet dispatch-ready prompt with no active
   turn; otherwise `WaitForBoundary` (never interrupts an in-flight turn).
   On `Restart` it requests a FRESH supervisor restart
   (`idle_watch.rs:777-803`: `restart_mode = "fresh"`,
   `restart_requested = true`), deduped per resolved binary.

2. **Re-resolution + fresh spawn (restart loop).**
   `run.rs:885-948` — the restart iteration re-reads CURRENT frontmatter,
   rebuilds the harness launch spec, and when
   `harness_change_forces_fresh_spawn(&harness.binary, &restart_spec.harness.binary)`
   (`decisions.rs:685-687`) is true, swaps in the new spec
   (`harness`, `base_args`, `resolved_env`), retires the old capability proof,
   clears `pending_adopt` so the OLD child is never adopted, and logs
   `agent_restart_performed ... action=spawn_fresh_harness`. INERT for an
   unchanged `agent:` (re-resolved binary matches ⇒ no swap).

### Does a change require a manual supervisor restart? — No.

The supervisor detects the change itself and self-restarts at the next quiet
boundary. The boundary discipline aligns with the existing AwaitDrain /
cycle-open interlock (`#supselfheal` recycle and `#midturn-recycle-resume`):
switch only at a turn/cycle boundary, never mid-turn.

### Config / knob

- `AGENT_DOC_AGENT_CHANGE_RESTART` (`project_controller.rs:90`), default ON,
  read via `project_controller::agent_change_restart_enabled`
  (`idle_watch.rs:553-554`).

### Logging (the switch is observable)

- `harness_change_detected` + `agent_restart_boundary_gate` (idle watch)
- `agent_restart_triggered ... action=request_fresh_restart` (idle watch)
- `agent_restart_performed ... action=spawn_fresh_harness` (restart loop)

## Phase 2 — Implementation

No production-code change was needed — the tightest correct fix already exists
(config-driven re-resolution at the restart boundary, single source of truth,
boundary-gated, fresh spawn of the new harness, logged). Forcing a parallel
change would have regressed the shipped design.

## Phase 3 — Tests added

Existing coverage already present:
- `decisions.rs::agent_change_restart_decision_policy` — boundary policy truth table.
- `decisions.rs::harness_change_forces_fresh_spawn_predicate` — change vs inert.
- `decisions.rs::restart_action_is_the_phase1b_trigger_value` — trigger wiring.

Added (close the operator-scenario gap explicitly, `harness.rs`):
- `from_context_picks_up_mid_session_agent_change_for_next_dispatch` — turn N
  resolves `claude`; operator edits frontmatter to `codex`; turn N+1
  re-resolution reflects `codex` AND `harness_change_forces_fresh_spawn` is true.
- `from_context_unchanged_agent_is_inert_across_turns` — unchanged `agent:`
  re-resolves to the same harness and does NOT force a fresh spawn.

## Phase 4 — Verify

`make check` GREEN (clippy + nextest).

## Residual risk / follow-up

- The pure policy + re-resolution path is fully unit-tested. The only piece that
  cannot be covered without a live PTY is the actual fresh re-spawn of the new
  harness child in a real tmux pane — that remains an `[operator-verify]` live
  eyeball (change `agent:` mid-session in a real editor, watch ops.log for
  `harness_change_detected` → `agent_restart_triggered` → `agent_restart_performed`
  and confirm the next dispatch lands in the new harness).
- No focused follow-up cycle is required; the feature is code-complete.
