> Supplement to [08-session-routing.md](08-session-routing.md)

# Session Actor Contract

This file freezes the phase-1 single-owner session actor contract while
documenting the phase-2 durable actor store boundary so later phases preserve
the same semantics.

## Authoritative intent

The normal path should behave as if one per-document session actor owns
authority, even while implementation still spans registry entries, supervisor
state, and session logs.

Authoritative fields:

- `document_id`
- `session_id`
- `generation`
- `pane_id`
- `window_id`
- `harness`
- `state`
- `last_transition`

## Generation semantics

- Generations are monotonic per document session and start at `1`.
- A new generation is created whenever normal-path control explicitly starts a
  new document session owner or rebinds authority to another pane.
- Harness child restarts inside the same owning supervisor do not create a new
ownership generation by themselves. This includes restarts initiated by
`/clear` in any harness (Codex, OpenCode): the supervisor auto-restarts the
child and the actor transitions through `Starting` → `Ready` without ever
entering `WaitingInput` or `Closed`. Route-level clear tracking
(`codex_hook::record_external_prompt_for_file`) is harness-agnostic so that
managed and dispatch-only routes can restart fresh after a tracked `/clear`
for both Codex and OpenCode.
- A supervisor binary hot-reexec that adopts the same surviving harness child is
a transport replacement, not a child restart or session start. Before
registering its replacement lease it must validate the existing
document/session/pane binding, then preserve the actor generation, state, and
last transition exactly. It emits no `session_start`; binding drift fails closed
without allocating a replacement generation.
- Same-generation lifecycle updates such as prompt readiness, dispatch-busy,
waiting-for-input, blocked, and closed must preserve the authoritative
generation while still updating `state` and `last_transition`.
- Legacy logs without explicit generation markers may infer historical
  generations from repeated `session_start` events for diagnostics, but new
  writes must emit explicit generation metadata.

## Ownership transition logging

Every ownership transition must emit a machine-readable log line with:

- `caller`
- `reason`
- `prior_generation`
- `new_generation`
- `old_pane`
- `new_pane`
- `old_window`
- `new_window`

Canonical event shape:

```text
ownership_transition caller=<caller> reason=<reason> prior_generation=<n> new_generation=<n+1> old_pane=<pane|none> new_pane=<pane> old_window=<window|none> new_window=<window|unknown>
```

Phase-1 callers:

- `start`
- `claim`
- `route`
- `sync`
- `register`

The stable requirement is the field schema, not the complete caller taxonomy.
Later phases may refine caller values without changing the field names.

## Current phase boundary

- `.agent-doc/state.db` is the durable per-project controller state store for
  actor records. The `documents` table carries the authoritative generation,
  pane/window binding, harness, state, and last transition id, while
  `actor_transitions` records each monotonic ownership or lifecycle transition.
- `.agent-doc/state.db` is the only actor and registry authority. Route, start,
  and sync ask the project controller for the actor binding; no actor JSON
  projection or compatibility reader participates in normal or recovery paths.
- Editor layout memory is controller state too. Sync reads and writes the
remembered column layout through `.agent-doc/state.db` `layout_states` rows.
No legacy layout file is imported or emitted.
- Actor-record `harness` values use canonical ids rather than raw binary names:
  `claude` normalizes to `claude-code`, `codex` stays `codex`, and empty values
  collapse to `default`. Normal-path route/start/sync checks must compare
  against that canonical identity so harness aliases do not strand a healthy
  authoritative actor. A different canonical harness blocks only while the
  existing record still has live, healthy, non-closed supervisor authority; stale
  cross-harness records may be replaced by a fresh start.
- Store updates must be monotonic and fail closed on generation regressions; a
  stale writer must not overwrite a newer generation.
- Same-generation state transitions must also fail closed when the caller's
  `session_id` or `pane_id` no longer matches the authoritative record.
- Controller IPC must be resilient to stalled peers. Server-side request reads
  and client-side response reads are bounded, each accepted client is handled
  independently, and readiness checks must release any idle controller stream
  before issuing a real RPC.
- Controller lazy launch must not trust a stale `current_exe()` path after a
  local binary replacement. If that path no longer exists, launch and bootstrap
  identity resolution fall back to the invoked command or `agent-doc` on `PATH`;
  only then may the client fail closed with the missing path in the diagnostic.
- The phase-3 supervisor path reports these actor transitions through the
  project controller IPC, not by independently rewriting actor files:
  `start_session`, `register_supervisor`, `prompt_ready`, `ipc_inject` /
  `auto_trigger_inject` busy dispatch, clean-exit `waiting_input`,
  `supervisor_halted`, and final `closed`.
- Lifecycle updates must include the caller's session id, pane id, and
  generation. The controller rejects stale lifecycle reports when any of those
  fields no longer match the authoritative actor record, and it updates
  `supervisor_leases` with the latest runtime state while preserving the
  registered supervisor pid/socket across state-only reports.
- Supervisor register/heartbeat reports for a newer session/generation may
  replace the actor record only when the current actor is `Closed`, the pane is
  unchanged, the reported generation is newer, and the report proves it comes
  from the same existing supervisor lease by PID or socket. This lets a
  same-supervisor Codex/OpenCode child restart clear closed state immediately
  without weakening stale session/pane/generation rejection for live actors.
- `prompt_visible_once` remains a child-lifecycle fact for restart heuristics, but
  prompt-driven `busy -> ready` recovery must fire after every later routed or
  auto-trigger dispatch that returns the same child to an idle prompt; it is not
  a spawn-only one-shot transition.
- The dispatch-ready wait barrier must repair a stale busy projection without
  waiting out the full `--wait-for-ready` timeout
  (`#run-agent-doc-busy-ready-deadlock`, `#snrun`). When the authoritative actor
  is projected `Busy` but the live pane proves a current-generation dispatch-ready
  prompt, the wait loop resolves to `Ready` on the first poll that proves the
  prompt and dispatches immediately, rather than spinning to the deadline and only
  then applying the post-wait `busy_projection_repaired_by_ready_prompt` promotion.
  Editor `Run Agent Doc` passes `--wait-for-ready 60`, so the prior
  spin-then-repair behavior blocked dispatch for ~60s (and sometimes fell through
  to focus-only with no dispatch) on a pane whose busy projection was already
  stale — an operator-visible deadlock. A `Busy` projection without a proven ready
  prompt still stays fail-closed (keeps waiting, then queues), per the
  direct-evidence rule.
- The phase-4 route path now consumes the authoritative actor binding through
  project controller IPC. Before submitting a managed or dispatch-only reopen
  into an actor-owned pane, route records a controller `dispatch` attempt for
  the current session id, pane id, generation, and command kind. Stale
  session/pane/generation requests fail closed before pane input is submitted,
  while accepted dispatches record the stage (`ready`, `busy_queue`, or
  `waiting_input_recovery`) in `dispatch_attempts`. A `starting` actor is not a
  dispatch stage: route must wait for it to report `ready`, then fail closed if
  it remains `starting` instead of sending tmux or supervisor input into the
  startup window.
- Accepted actor input is not enough proof that a routed prompt was submitted.
  The route layer records `route_submit_observation` for direct tmux submit and
  supervisor-backed dispatch-start proof checks. When the submitted trigger is
  still visible after the acceptance window, it logs `route_submit_issue
  issue=prompt_not_submitted`; when Codex hook tracking or OpenCode pane-state
  tracking requires dispatch proof but only acceptance is observed, it logs
  `route_submit_issue issue=accepted_without_dispatch_start_proof`.
- Direct-pane submit profiles get bounded bare submit-key re-submits
(`#jbcodexsubmit` / `#jbclaudesubmit`). Codex, Claude, OpenCode, and default
tmux submits send normalized text plus a named `Enter` key in one
`send-keys -t <pane> <text> Enter` operation. An empty first capture is not
itself acceptance proof: route waits for stable empty input so delayed Codex
composer paint/input can still become visible. When a submit times out with the
trigger visibly drafted, route sends up to three separate named `Enter` keys and
re-polls the acceptance window after each one, recording
`route_submit_resubmit ... action=submit_key key=Enter
result=accepted|still_visible attempt=N`. If Codex later has only
accepted-without-dispatch-start proof and the same trigger is visibly drafted,
route sends one late submit-key retry, records
`route_submit_late_resubmit ... cause=dispatch_start_unproven_prompt_visible`,
and rechecks dispatch-start proof. If the exact same routed trigger is already
visible in the composer before route sends anything, or if the visible
`agent-doc <relative-path>` draft resolves to the same file suffix as the routed
absolute-path trigger, route takes the same bounded submit-key path first instead
of appending another trigger; a later idle prompt below the trigger classifies it
as stale scrollback, not active draft input. This recovery never sends Enter
after the trigger disappears and remains capped on a genuinely stuck pane. A
successful full-trigger transport is an irreversible injection boundary: neither
an empty capture nor missing dispatch-start proof establishes that it had no side
effect, so route never sends the full payload again. Only the exact visible draft
can authorize the bounded bare-`Enter` recovery described above.
- Repeated dispatches are queue-first once a document actor is already busy.
  Route must first try to drain an open binary-owned closeout (`repair` /
  strict commit / `session-check`) so an idle console can accept the reroute
  immediately after the prior cycle is closed. The drain must reap completed
  tracked items across **all** surfaces (backlog, review, icebox) via full
  pending maintenance — not just the backlog-focused repair sub-step — so a
  deployed/completed `[x]` item left in review or icebox does not leave
  `session-check` interrupted and force the operator into a manual retry. If that drain is blocked, or if a
  dispatch-only reroute still cannot prove the authoritative actor is
  dispatch-ready, route must not inject a duplicate trigger. It appends the
  pending prompt-bearing request to `agent:queue`, creates the component when
  missing, sets `queue_active: true`, adds the `auto` queue attribute, and syncs
  the snapshot to that visible queue state. This makes the queued work
  inspectable and lets the normal Stop-hook/auto-queue path continue it after
  the current committed closeout, without hidden one-shot route state.
- The phase-5 sync/focus path also consumes that authoritative actor binding
  through the controller when the actor pane is still alive: sync
  rescues/reconciles layout around the actor-owned pane and focus selects it
and fails closed when that binding cannot be proven.
- The phase-6 operator surface is actor-backed as well: `session status`,
  `history`, `attach`, `restart`, `clear`, and `doctor` read or mutate the same
  authoritative record and supervisor IPC path instead of inventing separate
  tmux-only heuristics in the CLI or plugins.
- Operator commands must use the project controller as their actor boundary:
  status/history read controller-owned SQLite rows for the actor, transitions,
  supervisor lease, recent command attempts, and projection diagnostics; attach
creates the manual handoff generation through controller IPC; restart and clear record an
  `operator_<state>` acceptance or stage-specific rejection before touching the
  supervisor socket or tmux input.
- Operator clear/restart must add a second startup-window guard after
  controller acceptance and before any pane input or supervisor mutation. If
  the actor record, matching supervisor runtime, legacy session-scoped
  supervisor runtime, or matching supervisor lease is still `starting`, clear
  may proceed only with direct dispatch-ready composer evidence, while restart
  may proceed with either dispatch-ready composer evidence or the harness
  clean-exit restart prompt. Both commands must fail closed when the session
  document changed after the last committed response cycle, so a post-commit
  prompt edit cannot race a newly starting owned pane.
- Operator diagnostics must preserve the failed stage. Missing actors,
  blocked/closed actor states, stale projections, and recent failed operator
  attempts are durable controller diagnostics that editor plugins can display
  without scraping tmux state.
- `agent-doc gc`, plus normal `preflight`, `start`, and `sync` maintenance,
  may close a stale `starting` actor after one hour unless a live supervisor PID
  still has a fresh heartbeat for the same document generation. A live PID with
  a stale heartbeat is stuck startup state, not proof that boot is still making
progress. The controller transaction remains authoritative for that transition.
- The phase-7 repair boundary is now explicit: normal `sync` may capture
  diagnostics and fail closed on stash/window or closeout drift, but it does
  not run hidden layout rescue or closeout replay anymore. Those mutations live
  behind explicit repair surfaces such as `agent-doc repair <FILE>` and
  `agent-doc session doctor <FILE> --repair`.
- The phase-8 normal-path cleanup is now in place: route/start/sync no longer
  re-elect ownership from latest-open session logs, `registry_rebind`
  successors, or generic same-file process-tree scans. Those sources still
  matter for diagnostics and explicit repair, but they cannot silently reclaim
  authority away from the actor-backed path.
- Phase-9 verification must keep explicit regression coverage for generation
  monotonicity; phase-4 authoritative route-state handling (`ready` direct
  dispatch, `starting` / `busy` optimistic queueing, `waiting_input`
  fresh-restart recovery, and `blocked` / `closed` fail-closed rejection);
  stale session/pane rejection; blocked mixed-root layout preservation; and
  plugin operator surfaces that display exact session status, route
  `session clear` through the actor-backed command path, and preserve
  stage-specific dispatch failures in a durable diagnostics surface.
- Pane bindings and actor lifecycle state share the controller transaction.
  No compatibility JSON is emitted or read; state-store failures fail closed
  with durable diagnostics instead of creating a second mutable source.
- Session logs and `ops.log` are transition provenance outputs. The controller
  persists actor state first, then emits projection/log diagnostics from that
  committed state.
- Startup-miss, route, sync, and closeout recovery may read those logs, but
  they must not infer authority from multiple competing mutable sources once a
  newer generation is recorded.
- Dispatch-only degraded authoritative actor path: when the supervisor socket is
  unhealthy (Restartable, Halted, Unreachable, NoSocket) or actor_state is
  missing, and the authoritative actor pane still matches the registered
  dispatch pane or live owner binding, `route --dispatch-only` must keep that
  pane as the bounded recovery target instead of forcing the operator through a
  manual `agent-doc start <FILE>` rebind. Route logs
  `route_dispatch_only_authoritative_degraded_direct_pane`, including
  supervisor health and runtime actor state, records the controller dispatch
  attempt, and then uses the same direct-pane readiness, blocker, and proof
  checks as other editor reroutes before submitting. If those checks do not
  prove an idle prompt or dispatch-start proof where required, the route still
  fails closed before or after submit at the typed guard that failed. The pure
  degraded authority decision, runtime guard, degraded direct-submit log shape,
  authoritative actor ready-wait facts, delivery-action mode, retry budgets,
  direct-submit outcome, proof policy, and dispatch-start proof classifiers live
  in `flow::routed_reopen`; route-specific wrappers only map
  supervisor/controller facts into those FlowCore types and perform the
  tmux/supervisor/controller side effects. Pure unit tests must cover all
  supervisor health variants, matching/non-matching pane bindings,
  authoritative actor actions, retry budgets, and proof typing so degraded
  authority cannot bypass readiness or required dispatch-start proof.
