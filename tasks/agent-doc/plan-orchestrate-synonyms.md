# Plan: Orchestrate Command Synonym Recognition

**Pending:** `#osyn` in `tasks/agent-doc/agent-doc-bugs.md`
**Status:** Draft

## Problem

When a user writes "synchronous orchestra" or "run these in order" in a session document, the agent does not recognize these as triggers for `agent-doc orchestrate`. It falls back to manual sequential execution, missing the purpose-built tool.

## Goal

Teach the agent skill/runbook to map natural-language synonyms to `agent-doc orchestrate` modes so dispatch happens automatically.

## Synonym Table

| Mode | CLI | Synonyms |
|------|-----|----------|
| `sequential` | `orchestrate --mode sequential` | orchestra, chain, in order, one by one, sequentially, synchronous, run these sequentially |
| `parallel` | `orchestrate --mode parallel` | fan out, concurrent, at the same time, in parallel, simultaneously |
| `dag` | `orchestrate --mode dag` | dependency graph, depends on, after X do Y, fan in |

## Implementation Options

### Option A: Runbook (skill-side only)

Add a `runbooks/command-synonyms.md` in the agent-doc skill that the agent reads during dispatch. No binary changes.

- **Pro:** Zero binary changes, ships immediately, easy to iterate.
- **Con:** Agent must read and apply the table — LLM-dependent reliability.

### Option B: Binary `--from-natural` flag

Add a preprocessing step in the `orchestrate` binary that normalizes natural-language phrasing into `--mode` selection. The skill pipes user text through it.

- **Pro:** Deterministic matching, testable.
- **Con:** More implementation work, harder to iterate on synonyms.

### Option C: Hybrid — runbook + binary hint

Runbook teaches the agent to recognize synonyms and dispatch to `orchestrate`. Binary emits a suggestion ("did you mean `orchestrate --mode sequential`?") when it detects unrecognized natural-language task input.

- **Pro:** Best of both — agent handles happy path, binary catches misses.
- **Con:** Two surfaces to maintain.

## Recommended Approach

**Option A** for MVP. The synonym table is small (~15 entries) and the agent only needs to map to 3 modes. A runbook referenced from the skill definition is sufficient. Promote to Option C later if agent reliability is a problem.

## Implementation Steps

- [ ] 1. Draft `runbooks/command-synonyms.md` with the synonym table above
- [ ] 2. Add a reference in the skill definition (SKILL.md or CLAUDE.md) pointing to the runbook during orchestrate dispatch
- [ ] 3. Test: write "synchronous orchestra" in a session document and verify the agent dispatches to `orchestrate --mode sequential`
- [ ] 4. Test: write "fan out these tasks" and verify dispatch to `orchestrate --mode parallel`
- [ ] 5. Update agent-doc-bugs.md pending `#osyn` to done

## Files to Change

- `runbooks/command-synonyms.md` (new) — synonym table
- Skill definition referencing the runbook (exact path depends on where the skill is installed)
- No binary changes for MVP
