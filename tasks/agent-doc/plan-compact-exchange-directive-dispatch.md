# Plan: Compact Exchange Directive Dispatch

**Pending:** `#compactdispatch` in `tasks/agent-doc/agent-doc-bugs2.md`
**Status:** Implemented

## Problem

`src/session-share/tasks/claudescore-3.md` reproduced a directive-handling failure that is narrower than the older "replace, not append" bug:

- the live diff contained a bare `compact exchange` line
- `agent-doc run` still proceeded as a normal response cycle
- the closeout committed an ordinary `### Re:` response, left the directive visible, and did not actually compact the `agent:exchange` body
- the run reported success even though the requested binary compaction never happened

This means the run/write/finalize path currently accepts a compaction directive without proving that the turn took the compaction path or produced a compacted exchange result.

## Goal

When a user explicitly requests `compact exchange`, `agent-doc` must either:

- dispatch to the binary-owned compaction path, or
- fail closed before commit if the turn is about to complete as an ordinary response append.

## Desired Behavior

- a bare `compact exchange` directive in the user-authored diff is treated as an executable command, not ordinary conversational text
- `agent-doc run` / `write` / `finalize` do not report success unless the resulting `agent:exchange` body is actually compacted
- a successful compaction clears the directive from the live exchange and leaves one compact checkpoint summary or archive pointer
- if the tool cannot prove compaction happened, the cycle stays open or errors before commit instead of silently writing a normal `### Re:` response

## Implementation Steps

- [ ] Trace where diff classification currently notices direct imperative directives versus plain prompt text in `run`, `write`, `finalize`, and any Codex/Claude closeout helper
- [ ] Define the structural trigger precisely:
  - bare `compact exchange`
  - equivalent escaped transcript form inside `agent:exchange`
  - reject ambiguous mentions such as retrospective prose that merely quotes the phrase
- [ ] Decide the execution model:
  - preferred: route directly to the existing binary `compact` implementation
  - fallback: allow model-authored summary text only if the binary still performs the exchange replacement/archival itself
- [ ] Add a pre-commit invariant for compaction turns:
  - the directive line must not remain as unresolved user content
  - the exchange body must materially replace prior content rather than append another ordinary response
  - success must not depend on the agent "doing the right thing" in free-form prose
- [ ] Ensure the invariant composes with template + CRDT documents and submodule-owned session docs
- [ ] Decide how this interacts with the existing `#tailpatch` and "replace, not append" fixes so one guard does not mask another failure shape
- [ ] Add regressions for:
  - `agent-doc run` on a template/CRDT doc with a bare `compact exchange` diff
  - compaction with a custom summary message
  - submodule session doc reproduction matching `claudescore-3.md`
  - attempted ordinary response closeout when the directive is still unresolved
  - rerun after successful compaction yields `no_changes`
- [ ] Update spec/runbook text only after the executable path and fail-closed behavior are covered by tests

## Files Likely To Change

- `src/agent-doc/src/run.rs`
- `src/agent-doc/src/write.rs`
- `src/agent-doc/src/finalize.rs`
- `src/agent-doc/src/repair.rs`
- `src/agent-doc/src/compact.rs`
- `src/agent-doc/tests/`
- `src/agent-doc/SPEC.md`
- `src/agent-doc/runbooks/compact-exchange.md`

## Exit Criteria

- a bare `compact exchange` directive cannot silently complete as a normal response turn
- `claudescore-3.md` style reproduction either compacts through the binary path or fails before commit
- successful compaction leaves the document in a clean post-turn state with no unresolved directive residue
