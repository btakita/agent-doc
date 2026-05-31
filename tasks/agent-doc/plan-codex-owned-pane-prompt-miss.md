# Plan: Codex owned-pane direct invocation misses unresolved exchange prompt

## Status note (2026-05-31, #codex-owned-pane-prompt-miss-followups)

Base fix landed earlier (fail-closed early prompt-miss guard + `#recguard-abandon`
+ session-check backstop). Follow-up **phase 1 (structured result)** now landed:
`preflight` emits a typed `owned_pane_self_invocation` field (`PreflightOutput`
→ `run::OwnedPaneSelfInvocation`) with file/pane/session/actor, `kind`
(`unresolved_prompt` | `active_queue_head`), work excerpt, optional head id, and
the persistence command. The unresolved prompt is derived from the cycle's
prompt-target diff (not the boundary-keyed exchange detector) so it survives
preflight's post-commit boundary insertion; an unresolved prompt takes
precedence over an active auto-queue head. Tests:
`preflight_emits_owned_pane_self_invocation_for_unresolved_prompt` and
`preflight_owned_pane_self_invocation_absent_for_non_owner_pane`. Spec:
`specs/07-core-commands.md`.

**Phase 2a (guidance) now landed (2026-05-31):** the installed Codex agent-doc
guidance reads `owned_pane_self_invocation` and drives an in-pane response cycle
— a new bullet in `runbooks/harness-invocation.md` (Codex section, mirrored to
all four harness surfaces) tells the agent to NOT launch a recursive child, to
answer the `work_excerpt` for `kind: "unresolved_prompt"` / continue the
`active_queue_head` named by `head_id`, then persist via `persistence_command`
+ `session-check`, and to never let an active auto-queue head run ahead of an
unresolved exchange prompt. Regression: `tests/run_integration.rs`
`preflight_emits_owned_pane_self_invocation_for_active_queue_head` (complements
the existing `_for_unresolved_prompt` / `_absent_for_non_owner_pane` tests) locks
the structured contract for the queue-head case the guidance depends on.

**Remaining (gated — needs a live Codex pane):** (2b) a Codex Stop-hook live
regression asserting that, after the owned-pane diagnostic, Codex persists an
in-pane response body before sending final chat output (the deterministic
session-check backstop — `check_prompt_only_exchange_tail_guard` + the
`#recguard-abandon` terminal-abandon path — already landed in phase 1; this
remaining piece is a live Stop-hook assertion); (3) live Codex same-pane
verification with a real unresolved exchange prompt.

## Problem

Live repro from `src/boost-client/tasks/monsterrodholders.md` on 2026-05-30:

1. The exchange tail contained the unresolved user prompt:
   `Please assist in placing GA4 Analytics credentials in passage.`
2. The operator invoked:
   `agent-doc /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md`
   from the Codex pane that already owned that document.
3. `agent-doc run` pre-committed / reconciled the document, opened an empty
   run cycle, printed `Submitting to codex...`, then refused recursive dispatch:
   `recursive direct invocation would deadlock`.
4. The guard correctly avoided nesting another Codex child in the same pane and
   abandoned the empty preflight cycle, but the user prompt was not processed
   and no assistant response was written.

The existing `#recguard-abandon` fix made the recursive guard terminal instead
of wedging `session-check`, but the user-facing behavior is still wrong: a
valid unresolved exchange prompt can survive in the owner pane while the
operator only sees a restart/retry diagnostic.

## Desired Behavior

When a Codex-owned pane receives `agent-doc <FILE>` for the document it already
owns, agent-doc must not silently turn that request into a no-op for unresolved
exchange work.

Acceptable outcomes:

- Preferable: detect the owner-pane self-invocation before child dispatch and
  return a structured "process this prompt in the current owner pane" contract
  that Codex guidance can follow.
- Fallback: fail closed before snapshot/queue mutation, name the unresolved
  exchange prompt, and print the exact recovery path. The diagnostic must say
  not to retry the same direct command from the same pane.

In both cases, an unresolved exchange prompt must remain executable until a
response body is persisted through `agent-doc finalize` or `agent-doc write
--commit`.

## Proposed Fix

1. Move or duplicate the same-pane owner detection earlier in `run.rs`, before
   pre-commit/snapshot advancement and before `start_run_cycle`, when the
   document has unresolved exchange prompt text.
2. Reuse or extend the snapshot-independent unresolved prompt detector planned
   in `#prompt-preempts-auto-queue` so a prompt remains visible as user intent
   even if it was already baselined by a prior failed run.
3. Add an explicit `owned_pane_self_invocation` result shape to preflight/plan
   diagnostics:
   - file path
   - owner pane/session id/actor generation
   - unresolved prompt excerpt
   - required persistence command after response composition
4. Update the installed Codex agent-doc guidance so this result is handled as an
   in-pane response cycle:
   - read/plan the document
   - answer the unresolved prompt in the current assistant turn
   - persist via `agent-doc finalize` or `agent-doc write --commit`
   - do not launch another `agent-doc <FILE>` child from the same pane
5. Tighten `session-check` / Codex final-gate diagnostics:
   - if a recursive direct invocation was abandoned and unresolved exchange
     prompt text remains with no later response, report a missed-prompt
     recovery instead of treating the abandoned cycle as sufficient
   - ensure queue continuation does not run ahead of the unresolved exchange
     prompt
6. Keep the existing deadlock guard for true recursive child dispatch. The fix
   should not allow nested Codex children or reopen the original hang class.

## Tests

- Direct-run integration fixture:
  - Codex harness
  - `TMUX_PANE` equals the registered owner pane
  - exchange has an unresolved prompt
  - running `agent-doc <FILE>` produces `owned_pane_self_invocation` or the new
    fail-closed diagnostic before snapshot advancement
- Baselined prompt regression:
  - snapshot already contains the unresolved prompt
  - direct-run retry still reports the prompt as executable user intent
- Guard compatibility:
  - recursive same-pane direct invocation with no unresolved exchange prompt
    still abandons the empty cycle and passes the existing
    `#recguard-abandon` session-check test
- Codex guidance / hook regression:
  - after the owned-pane diagnostic, Codex is required to persist a response
    body before sending final chat output
- Queue precedence regression:
  - active `agent:queue auto` does not continue while the unresolved exchange
    prompt remains unanswered after a recursive guard

## Likely Files

- `src/agent-doc/agent-doc-orchestration/src/run.rs`
- `src/agent-doc/agent-doc-orchestration/src/preflight.rs`
- `src/agent-doc/agent-doc-orchestration/src/prompt_contract.rs`
- `src/agent-doc/agent-doc-orchestration/src/session_check.rs`
- `src/agent-doc/tests/run_integration.rs`
- `src/agent-doc/tests/codex_hook_integration.rs`
- installed/generated Codex agent-doc guidance

## Non-Goals

- Do not implement GA4 credential storage in this bug-fix cycle.
- Do not remove the recursive direct-invocation guard.
- Do not rely on the operator remembering to avoid same-pane direct runs; the
  binary and guidance should make the safe path explicit.
