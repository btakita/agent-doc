# Plan: Backlog Shadow Open Items

**Pending:** `#backlogshadow` in `tasks/agent-doc/agent-doc-bugs2.md`
**Status:** Draft

## Problem

Open backlog items can remain visibly present in a session document but drift out of the live `agent:backlog` block into parked/commented sections. Once that happens, normal backlog operations no longer track them as active work:

- reap only considers the live `agent:backlog`
- `session-check` and closeout logic treat the commented items as inert prose
- later commits can therefore leave genuinely open items absent from the operative backlog without any explicit completion event

The `claudescore-3.md` example showed this clearly:

- `#sraw5`, `#compaudit`, and `#missfit` were removed correctly after being completed
- the unfinished digest items `#sdig2`, `#sdig2fx`, and `#sredact` were not reaped as done
- instead, they had already drifted into the parked HTML comment under `# Session Digest`, so they vanished from the active backlog surface until manually restored

## Goal

Prevent open items from silently becoming “shadow backlog” entries outside the live `agent:backlog`.

## Desired Behavior

- if an open backlog-shaped line exists outside the live `agent:backlog`, `agent-doc` should surface it explicitly
- if the same item id appears both in the live backlog and in a parked/commented block, the tool should classify the shadow copy rather than silently ignoring it
- if open items are moved out of the live backlog during a write/finalize/preflight cycle, the command should either:
  - fail closed, or
  - emit a precise warning that active backlog items were stranded outside tracked backlog scope

## Implementation Steps

- [ ] Define what counts as a backlog-shaped shadow item outside `agent:backlog`
- [ ] Audit `preflight`, `session-check`, backlog diffing, and reap logic to see where open items outside the live block should be detected
- [ ] Add a detector for “open shadow backlog items” in session documents
- [ ] Decide the contract:
  - warning only for parked/reference copies that still have a live backlog entry
  - fail closed when an open item exists only in shadow form and not in the live backlog
- [ ] Add a repair or guidance path where possible
- [ ] Add regressions for:
  - open items only in the live backlog
  - open items duplicated in live backlog and parked comment
  - open items only in parked/commented sections
  - completed items in parked/reference sections that should remain inert
- [ ] Update command/spec docs once behavior is settled

## Files Likely To Change

- `src/agent-doc/src/preflight.rs`
- `src/agent-doc/src/session_check.rs`
- `src/agent-doc/src/diff.rs`
- `src/agent-doc/src/pending.rs`
- `src/agent-doc/specs/07-commands.md`
- `src/agent-doc/README.md`

## Exit Criteria

- unfinished items cannot disappear from active tracking merely because they drifted into a parked/commented section
- completed-item reap remains intact
- `claudescore-3.md` style digest-comment shadow items are detected and surfaced precisely
