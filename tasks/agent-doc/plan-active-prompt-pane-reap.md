# Plan — active Codex prompt panes must not be reaped during sync/rebind

**Pending:** `#prreap`
**Status:** Draft

## Problem

The latest `monsterrodholders.md` repro shows a stronger invariant break than the older busy-pane or visible-owner bugs: the previous tmux pane was reaped while the session was still active and while the user was typing directly into the Codex prompt.

Concrete evidence already visible in the persisted session history:

- `src/boost-client/.agent-doc/logs/a9421282-fd96-4943-9af5-3561ed5cb799.log` records repeated `supervisor_exit code=missing_pane ... reason=registered_pane_missing` followed immediately by `session_end origin=sync_missing_pane` and `session_superseded ... next_pane=...` for `tasks/monsterrodholders.md`.
- Those events are interleaved with otherwise healthy Codex cycles (`codex_start`, `document_cycle phase=...`, `commit_success`), which argues for synthetic ownership loss / rebind churn rather than a clean child shutdown.
- The user report adds the missing operator fact that the pane was not idle: the Codex prompt still had live drafted input when the old pane era was replaced.

That means the current recovery boundary is too permissive. Even if ownership proof is weak or ambiguous, agent-doc must not convert that uncertainty into destructive pane replacement when the pane still contains active user input.

## Desired Behavior

1. A tmux pane that is still alive and has drafted user input in the Codex prompt must not be reaped, superseded, or treated as `registered_pane_missing`.
2. Sync/start/route must distinguish:
   - dead pane
   - alive idle pane
   - alive busy pane
   - alive pane with drafted prompt input
3. If ownership proof weakens while drafted input is present, agent-doc must fail closed with diagnostics instead of starting a replacement pane era.
4. Session-log and ops-log provenance must make it obvious whether a pane was actually dead, merely unproven, or protected because drafted input was detected.

## Implementation Steps

- [ ] Reconstruct the exact `monsterrodholders.md` failure chain around the latest pane loss:
  - which command triggered sync/rebind
  - what pane ID was visible immediately before replacement
  - what `sessions.json`, session log, and tmux window layout said at that moment
  - whether the pane still had drafted prompt content in its visible buffer
- [ ] Audit the current replacement path in `src/agent-doc/src/sync.rs`, `src/agent-doc/src/route.rs`, and `src/agent-doc/src/start.rs`:
  - where `registered_pane_unowned` becomes "unresolved"
  - where unresolved becomes `repair_missing_registered_pane(...)`
  - where `session_superseded` / `registry_rebind` is emitted
  - whether any path checks for drafted prompt input before allowing replacement
- [ ] Define a shared "drafted input present" detector for Codex panes:
  - reuse or extend existing ready/busy prompt parsing instead of inventing a second heuristic
  - treat real drafted text differently from idle placeholder prompts
  - ensure reverse-i-search / queue footer / shell-search states do not masquerade as safe idle
- [ ] Add a hard fail-closed guard:
  - if pane is tmux-alive and drafted input is present, do not record synthetic `registered_pane_missing`
  - do not auto-start a replacement pane
  - surface an operator-facing reason that the pane was protected because live input was detected
- [ ] Tighten rebind/session-end semantics so protected panes do not get marked `session_end origin=sync_missing_pane` while still alive.
- [ ] Add regressions for:
  - alive registered pane with drafted Codex input
  - alive pane with weak ownership proof but visible drafted input
  - alive idle pane with no drafted input where recovery is still allowed
  - sync/layout churn attempting to replace a protected pane

## Files Likely To Change

- `src/agent-doc/src/sync.rs`
- `src/agent-doc/src/route.rs`
- `src/agent-doc/src/start.rs`
- `src/agent-doc/src/harness.rs`
- `src/agent-doc/src/startup_miss.rs`
- `src/agent-doc/specs/07-commands.md`

## Exit Criteria

- Reproducing the `monsterrodholders.md` shape no longer reaps a pane that still has drafted Codex input.
- Synthetic `registered_pane_missing` is impossible for a tmux-alive pane once drafted input protection triggers.
- Logs clearly say whether a pane was replaced because it was actually dead, or preserved because live input was present.
