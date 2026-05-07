# Plan: Trim `AGENTS.md` + `SKILL.md` context footprint without weakening `agent-doc`

**Pending:** `#instrcap` in `tasks/agent-doc/agent-doc-bugs2.md`
**Status:** Implemented

## Problem

The current `agent-doc` instruction surface is effective, but it is paying too much recurring
context cost on long-running Claude Code and Codex sessions:

- root `AGENTS.md` repeats large parts of the same workflow that the installed harness skill
  already owns
- `SKILL.md` has accumulated many recovery-path details that are important, but not all of them
  need to sit in the hot path of every turn
- runbook guidance, binary-owned invariants, and harness-specific invocation notes are not yet
  separated tightly enough, so the same contract is described in multiple places with only small
  wording differences

That makes every `agent-doc` turn reload more prose than necessary and increases the chance that
future fixes have to patch the same rule in several instruction surfaces.

## Goal

Keep the binary-owned closeout and fail-closed safety contract intact while shrinking the
always-loaded instruction surface for both Claude Code and Codex.

## Desired Outcome

- the top-level `AGENTS.md` / installed skill path becomes a compact routing layer:
  - trigger detection
  - mandatory command order
  - closeout invariants
  - when to consult specific runbooks
- detailed edge-case prose moves behind targeted runbook references unless it is needed on every
  turn
- Claude Code and Codex continue to share one generated core instruction body, with only minimal
  harness-specific invocation differences
- regressions prevent the hot-path instructions from expanding again without deliberate review

## Implementation Steps

- [x] Inventory the current hot-path instruction overlap across:
  - root `AGENTS.md`
  - source `src/agent-doc/SKILL.md`
  - installed harness copies
  - the most frequently cited runbooks
- [x] Classify each rule as one of:
  - always-needed turn contract
  - rare recovery procedure
  - explanatory background that can move out of the hot path
- [x] Rewrite the top-level surfaces into a shorter decision checklist that preserves:
  - binary-owned preflight/plan/finalize ordering
  - the no-manual-patchback rule
  - commit-boundary and session-check requirements
  - pending/backlog mutation rules that must stay visible every turn
- [x] Push long-form recovery details behind specific runbook links and verify the shortened text
  still points to the exact executable path for:
  - startup-miss recovery
  - orchestration handoff
  - commit-boundary exceptions
  - compact / compact exchange flows
- [x] Audit the generation/install path so Claude/Codex copies remain aligned from one shared
  source instead of drifting by manual edits
- [x] Add a lightweight regression or budget check for instruction size / duplicated sections so
  future growth is explicit
- [x] Re-run the `agent-doc` harness-install/update flow and verify the shortened instructions are
  still sufficient for a full preflight -> plan -> finalize cycle in both harnesses

## Files Likely To Change

- `AGENTS.md`
- `src/agent-doc/SKILL.md`
- `src/agent-doc/runbooks/harness-invocation.md`
- `src/agent-doc/runbooks/commit.md`
- generator/install surfaces that materialize harness-specific copies
- regression coverage for generated instruction parity or size budgets

## Exit Criteria

- `agent-doc` retains the same safety-critical closeout behavior after the instruction trim
- the always-loaded instruction surfaces are materially shorter and less repetitive
- Claude Code and Codex continue to receive the same core contract from one maintained source
- future instruction bloat is caught by regression coverage instead of silently accumulating
