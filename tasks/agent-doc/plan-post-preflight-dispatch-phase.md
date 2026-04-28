# Plan: Post-Preflight Planning and Dispatch Phase

**Pending:** `#1g42` in `tasks/agent-doc/agent-doc-bugs.md`
**Status:** Draft

## Problem

`agent-doc preflight` now emits structured facts such as diff shape, command
detection, pending signals, and orchestration intent. That helps, but the
current skill flow still relies on the live model to read those facts, infer
what matters, and choose the next action directly from prose guidance.

That leaves the same failure mode seen in the orchestration miss:

- the signal exists
- the model sees the signal
- dispatch still depends on prompt-following instead of an explicit phase

At the same time, moving semantic planning into `preflight` would make
`preflight` slower, less deterministic, harder to test, and more
harness-dependent.

## Goal

Add a separate post-preflight planning/dispatch phase that:

- consumes the structured `preflight` result
- lets the model do semantic planning after the deterministic binary pass
- produces an explicit, reviewable dispatch record before repo work starts
- gives the skill/orchestrator a narrower contract to enforce

## Non-Goals

- making `agent-doc preflight` model-dependent
- replacing binary-owned command detection with free-form prompt prose
- solving every harness-specific orchestration API in the first iteration

## Desired Contract

The new phase should answer, in a structured way:

1. Which user prompts in the changed exchange tail must be answered this cycle?
2. Which repo actions must run before response persistence?
3. Which binary commands are mandatory (`orchestrate`, `claim`, `compact`,
   `finalize`, pending ops, etc.)?
4. Which follow-up items should be captured in `agent:pending`?
5. Whether the cycle should continue in-place or hand off to another binary
   surface such as `agent-doc orchestrate`

If the phase cannot produce that record confidently, it should fail closed and
surface the blocker instead of silently letting the skill improvise.

## Options

### Option A: Skill-only planning prompt

After `preflight`, the skill forces the model to produce a compact planning
record in a fixed schema, then executes from that record.

- **Pro:** Fastest path; no new binary command.
- **Con:** Enforcement still lives mostly in prompt discipline.

### Option B: Binary-owned `agent-doc plan` command

Add a dedicated command that accepts the `preflight` context, runs the planning
prompt through the harness model, and validates the output shape before handing
control back to the skill.

- **Pro:** Strongest contract boundary; testable and harness-visible.
- **Con:** Larger implementation; introduces a new binary/model integration
  surface.

### Option C: Rely on raw preflight JSON only

Keep extending `preflight` and expect the skill to reason directly from the raw
JSON plus prose instructions.

- **Pro:** No new phase to build.
- **Con:** Preserves the exact failure mode that `#1g42` is trying to remove.

## Recommended Approach

Start with **Option A** as a constrained planning phase, but design it so it can
graduate to **Option B** without changing the contract.

Concretely:

- define a narrow planning-record schema in the skill/runbook
- require the model to emit that record immediately after `preflight`
- make the dispatcher consume the record instead of the raw exchange/prose
- reject missing required fields or contradictory actions

This keeps `preflight` deterministic while giving the project a real phase
boundary now. If the schema proves stable, it can later move behind a dedicated
`agent-doc plan` command.

## Proposed Planning Record

Minimum fields:

- `prompt_targets`: ordered list of prompts that must be answered
- `repo_actions`: ordered list of concrete repo work to do before persistence
- `required_commands`: binary commands that must run before any free-form reply
- `pending_mutations`: pending adds/edits/done/gate actions for this cycle
- `handoff`: `none | orchestrate | compact | claim | other`
- `blockers`: concrete blockers that prevent execution

The dispatcher should execute from this record, not from ad hoc interpretation
of the raw diff.

## Implementation Steps

- [ ] 1. Document the planning-record schema and phase boundary in the agent-doc
      skill/runbooks
- [ ] 2. Update the skill workflow so `preflight` is followed by a required
      planning/dispatch step before any repo work or free-form response
- [ ] 3. Make orchestration and slash-command dispatch consume the planning
      record instead of re-reading the raw diff/prose directly
- [ ] 4. Add regression coverage for missed-dispatch cases like natural-language
      orchestration requests to prove the new phase catches them
- [ ] 5. Decide whether the first implementation stays skill-side or moves into
      a dedicated `agent-doc plan` binary command
- [ ] 6. Once stable, update the session/runbook docs so the phase is part of
      the normal contract rather than advisory guidance

## Files Likely to Change

- `src/agent-doc/SKILL.md`
- `src/agent-doc/runbooks/harness-invocation.md`
- `src/agent-doc/runbooks/model-tier-gate.md`
- `src/agent-doc/runbooks/streaming-checkpoints.md`
- `src/agent-doc/src/preflight.rs` only if new structured fields are needed as
  inputs to the planning phase

## Open Questions

- Should the planning record be markdown, JSON, or another compact structured
  format?
- Should the dispatcher be allowed to repair an incomplete planning record, or
  must that always fail closed?
- How much of the record should be binary-owned vs. harness-owned when multiple
  harnesses are supported?
