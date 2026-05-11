# Plan: Dispatch-only Degraded Authoritative Actor Path Skips Capability Proof Gate

## Problem

When `route --dispatch-only` encounters an authoritative actor whose supervisor is degraded (unreachable socket, no socket, missing actor state), the degraded fallback path passes `skip_capability_proof: true` to `dispatch_only_reopen_existing_pane`. This bypasses the managed capability proof gate entirely, logging `reason=degraded_supervisor_unreachable`.

This is intentional per spec (`07-session-tmux-commands.md` line 49): the degraded fallback must skip the managed capability proof gate because the supervisor is unreachable and cannot re-establish proof. A missing or previously-failed proof must not block dispatch when the pane is still the authoritative binding.

However, there are no pure unit tests covering the decision functions (`dispatch_only_can_use_degraded_authoritative_actor`, `authoritative_actor_dispatch_guard_reason`) that gate this path. All existing tests are `#[ignore]` tmux integration tests.

## Changes

### 1. Pure unit tests for decision functions (`route.rs`)

- `dispatch_only_can_use_degraded_authoritative_actor_returns_true_when_registered_matches` — pane matches registered dispatch pane
- `dispatch_only_can_use_degraded_authoritative_actor_returns_true_when_live_owner_matches` — pane matches live owner pane
- `dispatch_only_can_use_degraded_authoritative_actor_returns_false_when_no_match` — pane matches neither
- `authoritative_actor_dispatch_guard_reason_returns_none_for_healthy_with_state` — healthy supervisor + actor state = no guard
- `authoritative_actor_dispatch_guard_reason_returns_reason_for_unhealthy_supervisor` — all unhealthy variants (Restartable, Halted, Unreachable, NoSocket)
- `authoritative_actor_dispatch_guard_reason_returns_reason_for_missing_actor_state` — healthy supervisor but no actor state
- `authoritative_actor_dispatch_target_eligible_true_only_when_no_guard_reason` — eligibility matches guard reason

### 2. Spec update (`08a-session-actor-contract.md`)

Add a section documenting the degraded authoritative actor dispatch path and its capability proof exemption, referencing the existing spec language in `07-session-tmux-commands.md`.

## Scope

- No production code changes — the behavior is correct and already spec-compliant
- Pure unit tests only — no tmux integration tests added
- Spec documentation update
