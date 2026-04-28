# Planning / Dispatch

Use this runbook immediately after `agent-doc preflight <FILE>`.

## Goal

Turn the diff/preflight signals into an explicit execution contract before repo work starts.

## Command

Run:

```bash
agent-doc plan <FILE>
```

The command emits a structured planning record as JSON.

## Planning Record

- `prompt_targets` — ordered prompts that must be answered this cycle
- `repo_actions` — concrete repo work to complete before response persistence
- `required_commands` — binary/harness commands that must run this cycle
- `pending_mutations` — pending items that must be resolved before persistence
- `handoff` — `none | orchestrate | compact | claim | other`
- `blockers` — concrete blockers that mean the cycle should fail closed

## Dispatch Rules

1. Run the planning phase after preflight and before repo work.
2. Execute `required_commands` and respect `handoff` before free-form response generation.
3. If `handoff=orchestrate`, use the emitted `agent-doc orchestrate ...` command instead of manually simulating the batch.
4. Execute `repo_actions` before `finalize` / `write --commit`.
5. Resolve `pending_mutations` in the same cycle so pending state does not drift.
6. If `blockers` is non-empty, surface the blocker and stop instead of freelancing around it.

## Notes

- The current implementation is intentionally deterministic: it reuses the same binary-owned diff classifiers that preflight already depends on.
- The point of the phase is the contract boundary, not hidden model improvisation.
- If the schema needs richer semantics later, keep the same record shape so the skill contract stays stable.
