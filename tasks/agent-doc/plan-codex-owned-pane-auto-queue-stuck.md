# Plan: Codex owned-pane auto-queue self-invocation leaves turn stuck

**Pending:** `#codex-owned-pane-auto-queue-stuck`
**Status:** Fallback landed (gated on live Codex verify)

## Status note (2026-05-31)

The fail-closed early handoff guard (the plan's acceptable "Fallback" outcome)
landed: `run.rs` now detects an owner-pane self-invocation with a ready active
auto-queue head (`queue_continuation::detect`) and bails **before** pre-commit /
`start_run_cycle` with `owned_pane_queue_handoff_diagnostic` — naming the live
head + id, the in-owner-turn `finalize` / `write --commit` recovery, and warning
against re-running the same direct command. No queue/boundary/snapshot drift, no
abandoned cycle. Tests: `owned_pane_queue_handoff_diagnostic_names_head_and_recovery`
(unit) and `codex_owned_pane_active_auto_queue_hands_off_without_drift`
(integration; replaces the old recursive-abandon-for-queue test, which encoded
the pre-fix behavior). The recursive deadlock guard is unchanged for non-queue
nested dispatch.

**Remaining (gated):** the "Preferable" structured `owned_pane_queue_handoff`
result that Codex guidance consumes as the turn's prompt (items 3–4), the
`session-check --codex-final-gate` abandoned-cycle interrupt (item 5 — largely
covered already because the early bail keeps the head live and
`queue_continuation_required=true`), and live Codex same-pane verification
(tests #1–#5 in a real env).

## Problem

Live repro from `src/boost-client/tasks/monsterrodholders.md` on 2026-05-31:

1. The document had `queue_active: true`, `<!-- agent:queue auto -->`, and a
   ready queue head:
   `re [#374n]: "Ads-side shopping_product coverage is still not at parity"...`
2. The operator invoked:
   `agent-doc /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md`
   from the Codex pane that already owned that document.
3. `agent-doc` first reconciled an already-committed snapshot, then re-read the
   document, saw the active queue diff, printed `Submitting to codex...`, and
   hit the recursive direct-invocation guard:
   `recursive direct invocation would deadlock`.
4. The guard correctly avoided nesting a Codex child in the same pane, but the
   queued prompt was not handed back to the owning Codex turn and no response was
   written.
5. The working tree was left with queue bookkeeping drift (`queue_active: true`
   and a changed `agent:boundary` id), while the queue head remained live. From
   the operator perspective, the turn is stuck: re-running the same direct
   command from the same pane repeats the guard instead of advancing the queue.

This is distinct from `#recguard-abandon`, which covers closing the empty
preflight cycle after the recursive guard. Here the cycle is terminal, but the
active auto-queue prompt still has no executable owner-pane handoff.

## Desired Behavior

When `agent-doc <FILE>` is invoked from the Codex pane that already owns the
document and the document has a ready active auto-queue head, agent-doc should
not attempt child dispatch and should not leave the operator with a retry loop.

Acceptable outcomes:

- Preferable: return a structured owner-pane queue handoff that Codex guidance
  can treat as the current turn's prompt, including the exact queue head and the
  required `finalize` / `write --commit` closeout path.
- Fallback: fail closed before mutating queue/snapshot/boundary state, name the
  active queue head, and print an exact recovery instruction that says not to
  retry the same command from the same pane.

In both cases, the auto-queue head must remain actionable until a response body
is persisted and queue consumption is proven.

## Proposed Fix

1. Move active owner-pane self-invocation detection earlier in `run.rs`, before
   `Submitting to codex...`, before child dispatch, and before boundary-only
   cycle mutations when no response will be captured.
2. Reuse the active queue detector from the Codex final-gate work:
   `queue_active: true`, `agent:queue auto`, ready non-stop head, and no
   time-gate/modified-head halt.
3. Add a typed result such as `owned_pane_queue_handoff` with:
   - file path
   - owner pane/session id/actor generation
   - queue head excerpt and optional resolved id
   - closeout requirement (`agent-doc finalize` or `agent-doc write --commit`)
   - instruction not to run recursive `agent-doc <FILE>` again from that pane
4. Persist durable queue-target proof for handoff cycles so a later
   `finalize` / `write --commit` can consume the exact queue head without
   weakening id-backed queue safety.
5. Teach `session-check --codex-final-gate` to interrupt when an abandoned
   recursive direct invocation left an active queue head with no later response,
   reporting the owner-pane handoff recovery instead of treating the terminal
   abandoned state as sufficient.
6. Keep the recursive deadlock guard for true nested child dispatch. The new
   path should bypass child dispatch only when the current pane already owns the
   document and the active queue head can be handed to that owner.

## Tests

1. Direct-run regression: Codex harness, `TMUX_PANE` equals the registered owner,
   active auto queue has a ready head, and `agent-doc <FILE>` returns
   `owned_pane_queue_handoff` before `Submitting to codex...`.
2. No-drift regression: the same owner-pane handoff does not create
   boundary-only or queue bookkeeping drift when no response is written.
3. Closeout regression: after the handoff, a response with the captured queue
   target proof can consume the exact head and expose the next queue prompt.
4. Final-gate regression: if no response is written after the handoff,
   `session-check --codex-final-gate` exits nonzero and names the still-live
   queue head.
5. Compatibility regression: direct invocation from a non-owner shell still
   dispatches normally, and recursive same-pane invocation with no unresolved
   prompt/queue work still follows the terminal `#recguard-abandon` path.

## Likely Files

- `src/agent-doc/agent-doc-orchestration/src/run.rs`
- `src/agent-doc/agent-doc-orchestration/src/queue.rs` or a shared queue
  continuation module
- `src/agent-doc/agent-doc-orchestration/src/session_check.rs`
- `src/agent-doc/tests/run_integration.rs`
- `src/agent-doc/tests/codex_hook_integration.rs`
- generated Codex agent-doc guidance/runbooks

## Related

- `#recguard-abandon`: terminal cleanup for empty recursive direct-invocation
  cycles.
- `#codex-owned-pane-prompt-miss`: same owner-pane shape, but for unresolved
  exchange prompts instead of auto-queue heads.
- `#free-text-queue-owner-consume`: queue consumption proof after an owner-pane
  response exists.
- `#codex-auto-queue-stalled-final-gate`: final-answer gate when a clean
  closeout still has a remaining auto-queue continuation.

## Non-Goals

- Do not remove the recursive direct-invocation guard.
- Do not broadly consume id-backed queue heads from response headings alone.
- Do not implement this as instruction-only behavior; the handoff/final gate
  must be binary-owned and regression tested.
