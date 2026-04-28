# Plan: Preflight `no_changes` Swallows a Live User Prompt

**Pending:** `#nodiffswallow` in `tasks/agent-doc/agent-doc-bugs2.md`
**Status:** Implemented

## Problem

`tasks/monsterrodholders.md` reproduced a fail-open preflight path:

- `HEAD` before the `agent-doc` run did **not** contain the new prompt `Please analyze failed orders and bot traffic on monsterrodholders.com. #next-steps`
- the live document **did** contain that new prompt, inserted just before the `agent:boundary` marker in `agent:exchange`
- `agent-doc preflight` logged `changes detected, computing unified diff`
- the same preflight cycle also auto-repaired and committed a stale completed backlog item
- after that cycle, the saved snapshot and live document both contained the new prompt, but preflight still returned `no_changes: true`
- the harness then correctly stopped without answering, because `agent-doc` had claimed there was nothing to do

That means a user-authored prompt-bearing edit was absorbed into the snapshot/closeout path without producing either:

- a visible response turn, or
- a fail-closed error that the cycle was ambiguous

## Goal

If a live document contains a new user-authored prompt that is not in the prior committed state, preflight must never let repair/commit/snapshot maintenance turn that into `no_changes: true`.

## Desired Behavior

- prompt-bearing edits in `agent:exchange` remain actionable even when they are plain inserted text near the exchange boundary rather than a clean `❯ ...` transcript line
- if repair/commit mutates the document in the same cycle, preflight still proves whether unresolved user input remains relative to pre-repair `HEAD`
- `no_changes: true` is only allowed when:
  - the live doc has no unresolved user-authored delta relative to the owning repo `HEAD`, and
  - snapshot/baseline maintenance did not absorb new prompt text
- if the tool cannot prove that, it must fail closed before the harness reports "nothing changed"

## Implemented Scope

- `repair_completed_backlog_items()` now mirrors stale completed-backlog reaps into the snapshot surgically from the snapshot's backlog/archive components instead of re-saving the whole live document state.
- That keeps prompt-bearing exchange drift outside the snapshot, so the next `preflight` diff still sees prompts inserted immediately before `agent:boundary`.
- Added direct regression coverage for the `repair` path and end-to-end regression coverage for the `preflight` path where stale backlog reap and a live prompt coexist.
- Updated the command spec and version notes to make the "do not absorb live prompt-bearing exchange drift into the snapshot" contract explicit.

## Files Likely To Change

- `src/agent-doc/src/preflight.rs`
- `src/agent-doc/src/repair.rs`
- `src/agent-doc/src/git.rs`
- `src/agent-doc/src/write.rs`
- `src/agent-doc/tests/`
- `src/agent-doc/specs/07-commands.md`
- `src/agent-doc/runbooks/document-format.md`

## Exit Criteria

- the `monsterrodholders.md` reproduction cannot report `no_changes: true` while a fresh user prompt is still unanswered
- snapshot/baseline maintenance cannot silently absorb prompt-bearing edits
- the harness either receives an actionable diff to answer or a fail-closed diagnostic
