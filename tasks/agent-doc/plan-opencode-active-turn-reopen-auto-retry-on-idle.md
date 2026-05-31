# Plan: dispatch-only reopen auto-retry once an active OpenCode/Codex turn goes idle

## Status

Design plan only. Implementation + live-pane verification are **gated** — the
behavior touches live OpenCode/Codex pane transitions that cannot be proven from
deterministic tests alone. Tracked by `#opencode-active-turn-reopen-auto-retry-on-idle`
(`[recommended]`, explicitly lower priority than the live-verify backlog items).

## Symptom / motivation

When a plain dispatch-only `Run Agent Doc` reopen (no prompt-bearing work) targets an
authoritative actor whose pane shows an **active turn** (`busy` projection proven by a
live busy cue, not a stale projection), route currently fails closed with the
busy-not-ready diagnostic after focusing the pane. This is the correct safety boundary
today (`specs/08-session-routing.md`, the `**Authoritative actor dispatch:**` invariant,
and `route.rs` `dispatch_only_focus_only_should_fail_closed` →
`route_dispatch_only_authoritative_actor_busy_focus_only_not_dispatched`).

The operator must re-click `Run Agent Doc` (or rerun `agent-doc route <FILE>`) once the
turn ends. The refinement: optionally watch the pane and auto-fire the **bare reopen**
once it returns to a dispatch-ready / idle composer, so the operator does not retry by
hand.

## Hard invariants (must not regress)

1. **Plain reopen only.** This path is reachable only when there is *no* prompt-bearing
   work (`prompt_bearing_marker.is_none()` / `has_prompt_bearing_work == false`). It must
   never queue a prompt into `agent:queue auto`, and never inject prompt text. A reopen
   that carries prompt-bearing drift keeps the existing busy → queue-supersession path.
2. **No-inject-into-active-turn.** Never send any input while the busy cue is still
   present. The retry only fires after the live capture proves a harness-specific
   dispatch-ready prompt in the *current* generation — the same proof gate the
   `starting`/`busy` paths already require (`#snrun` direct-evidence rule). OpenCode's
   idle splash (`Ask anything...`, build-plan/footer chrome) counts as dispatch-ready per
   the existing stash-routing invariant.
3. **Bounded, fail-closed.** The idle wait uses a bounded budget (reuse the
   harness-specific ready/recovery retry budget, e.g.
   `dispatch_only_starting_pane_recovery_retry_budget`). On timeout it falls back to the
   existing busy fail-closed diagnostic — never an unbounded poll, never a silent success.
4. **Opt-in.** Gate behind config / flag so default behavior stays fail-closed (the
   operator-retry contract today). Candidate: frontmatter / `.agent-doc/config.toml`
   `route_dispatch_only_idle_retry = true`, or a `route --wait-for-idle` flag. Default off.
5. **Single authoritative generation.** Re-resolve the actor generation after idle before
   dispatch; if a different pane/generation became authoritative during the wait, restart
   resolution rather than dispatching into a superseded pane (mirror the starting-actor
   timeout keying by document + pane + generation).

## Implementation sketch

- **FlowCore classifier (pure, testable):** add a `flow::routed_reopen` outcome, e.g.
  `BusyPaneAutoFixOutcome::WaitForIdleThenRetry { budget }`, selected only when
  (no prompt-bearing work) ∧ (opt-in enabled) ∧ (busy is a real active turn, not a stale
  projection). This is unit-testable with fixed inputs (config, prompt-bearing flag,
  blocker reason) and needs no live tmux.
- **route.rs I/O boundary:** on that outcome, poll `tmux capture-pane` against
  `harness.has_busy_cue()` / dispatch-ready prompt detection on the existing
  100ms-style debounce cadence until idle-with-ready-prompt or budget exhaustion, then
  call `dispatch_only_reopen_existing_pane(...)` (the same bare-reopen path the
  interrupt-recovery and fresh-restart branches already re-enter). On timeout, emit the
  existing busy fail-closed diagnostic.
- **Logging:** new `route_dispatch_only_busy_wait_for_idle_started` /
  `..._retry_dispatched` / `..._timeout_fail_closed` ops-log lines plus a mirror-mode
  `flow_event flow=routed_reopen stage=busy_idle_retry outcome=...` so ops-summary can
  group it.

## Verification

- **Deterministic (in-repo):** unit tests for the `WaitForIdleThenRetry` classifier
  (opt-in on/off, prompt-bearing vs plain, real-busy vs stale-projection, generation
  drift during wait). SimWorld coverage for the busy→idle pane transition driving the
  retry, if the SimWorld harness can model a busy-cue→ready-prompt capture sequence.
- **Live (gated, operator-present):** real OpenCode and Codex panes mid-turn, confirm the
  reopen fires exactly once on idle, never injects during the active turn, and falls back
  to fail-closed on timeout. This is the gate that keeps the item out of `--done`.

## Files

- `agent-doc-orchestration/src/flow/routed_reopen.rs` — new outcome + classifier + tests
- `agent-doc-orchestration/src/route.rs` — busy-pane wait/poll + re-dispatch wiring
- `src/config.rs` / `src/frontmatter.rs` — opt-in flag plumbing
- `specs/08-session-routing.md` — promote the contract to a normative invariant **after**
  implementation lands (specs describe implemented behavior, so the spec entry waits)
- README.md / SPEC.md / installed harness surfaces — align once implemented
