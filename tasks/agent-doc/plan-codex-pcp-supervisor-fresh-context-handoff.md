# Plan - Codex PCP/supervisor fresh-context handoff

Tag: `[clean-session]`

## Problem

A Codex Stop hook can correctly determine that a queue continuation needs fresh
context, but the current failure path tells the operator to run `/clear` and
re-invoke `agent-doc` manually. That is the wrong ownership boundary for an
active agent-doc queue: when the current Codex turn cannot safely do the next
step, the project controller/supervisor should own the fresh-context handoff.

Concrete repro captured in `tasks/agent-doc/agent-doc-bugs2.md`:

- `tasks/professional/equityfundingsource.md` had an active `agent:queue auto`.
- The Stop hook said fresh context was required before `#zwws-new-deal-email`.
- Codex answered that it could not execute `/clear` from inside the loaded
  context and asked the operator to clear/reinvoke manually.

## Goal

When Codex cannot complete an agent-doc continuation inside the current turn
because the next step requires fresh context, agent-doc should enqueue or route
that continuation through PCP/supervisor automatically.

## Implementation Outline

1. Identify the Stop hook branch that emits the fresh-context-required feedback
   for active `agent:queue auto` documents.
2. Replace the operator-punt path with a supervisor-owned handoff:
   - record that the current turn is stopped due to fresh-context requirement,
   - enqueue one deduped fresh-context trigger for the same document,
   - deliver either `/clear` + `agent-doc <FILE>` at the between-turn boundary
     or a supervisor restart/rebind that preserves the original Codex launch
     contract.
3. Reuse the existing between-turn dedupe and clean-session machinery instead of
   adding a second ad hoc queue injector.
4. Emit explicit proof in ops/session logs, for example:
   `codex_fresh_context_handoff via=supervisor file=... head=... result=queued`.
5. Ensure the current assistant response does not invite the operator to do the
   manual `/clear` when the supervisor handoff was accepted.

## Acceptance

- A fixture or SimWorld test covers the Stop-hook fresh-context-required branch
  and proves it queues exactly one supervisor handoff.
- A repeated handoff does not concatenate duplicate `/clear` or `agent-doc`
  triggers.
- The handoff preserves the original Codex launch contract and permissions.
- The equityfundingsource-style repro continues from a fresh context without a
  manual operator `/clear`.

## Related Work

- `tasks/agent-doc/plan-between-turn-trigger-dedup.md`
- `tasks/agent-doc/plan-codex-clear-permissions.md`
- `tasks/agent-doc/plan-jb-clear-session-context-submit.md`
- `tasks/agent-doc/plan-queue-dispatch-flood.md`
