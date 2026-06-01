# Plan - submodule response commit proof and parent-pointer reporting

**Pending:** `#submodule-response-commit-proof`
**Status:** Draft

## Problem

Live repro, 2026-06-01, from a Codex-owned pane on:

`src/boost-client/tasks/monsterrodholders.md`

The operator invoked:

```bash
agent-doc /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md
```

`agent-doc` refused recursive same-pane dispatch with:

```text
recursive direct invocation would deadlock
```

The assistant then fell back to manual document editing and committed the target
document inside the owning `src/boost-client` submodule as:

```text
6cbff1d agent-doc response: reduce ad spend
```

However, from the superproject root, `git status --short` still showed:

```text
M src/boost-client
```

The final response said the response was committed, but it did not distinguish:

- the target session document was committed in its owning submodule
- the parent repo still had an uncommitted submodule pointer change
- the response was not persisted through a binary-owned `agent-doc write` /
  `finalize` closeout because the recursive invocation had already failed

That leaves the operator with a reasonable "response was not committed" report,
because the visible workspace still advertises dirty response-related state.

## Desired Behavior

For session documents inside nested repos or submodules, agent-doc closeout
should make commit ownership explicit and verifiable:

- successful response closeout must prove the target document commit in the
  owning git root
- if a parent repo is dirtied only by the submodule pointer, the closeout must
  report that separately instead of saying simply "committed"
- if policy requires the parent pointer to be committed for the current
  workspace, agent-doc must either commit it through an explicit approved path
  or fail closed with the exact parent commit command
- a manual fallback after recursive same-pane dispatch must not be allowed to
  present a normal agent-doc commit closeout unless `write --commit` /
  `finalize` or an equivalent session-check proof validates it

## Plan

1. Reproduce the nested repo shape with a fixture:
   - superproject contains a submodule
   - target session doc lives inside the submodule
   - response is committed inside the submodule
   - parent root remains dirty only by the submodule pointer
2. Audit current closeout diagnostics for:
   - owning git root detection
   - parent git root detection
   - submodule pointer dirtiness
   - final response wording emitted by Codex/Claude guidance
3. Add a commit-proof classification:
   - `target_committed_clean`: target owning repo clean after commit
   - `target_committed_parent_pointer_dirty`: target repo clean, parent pointer
     dirty
   - `target_dirty_commit_missing`: response visible but target repo dirty
   - `manual_fallback_unverified`: response was patched outside agent-doc
     closeout and still needs `write --commit` / `session-check`
4. Teach `session-check` and final-gate diagnostics to surface the exact state:
   - target commit hash and owning repo
   - parent repo pointer status if applicable
   - whether the parent pointer was intentionally left for a separate commit
5. Add recovery text for the parent-pointer case:
   - if parent commit is required: stage only the submodule path and commit
   - if parent commit is intentionally deferred: say the target doc commit is
     complete but the parent workspace remains dirty by design
6. Ensure recursive same-pane fallback guidance directs the assistant to a
   binary-owned recovery path instead of manual patch + ambiguous commit report.

## Likely Files

- `src/agent-doc/agent-doc-orchestration/src/git.rs`
- `src/agent-doc/agent-doc-orchestration/src/write.rs`
- `src/agent-doc/agent-doc-orchestration/src/finalize.rs`
- `src/agent-doc/agent-doc-orchestration/src/session_check.rs`
- `src/agent-doc/agent-doc-orchestration/src/run.rs`
- Codex / Claude generated agent-doc guidance and runbooks
- integration fixtures covering nested repos and submodules

## Tests

- Submodule target commit test: response write commits the target document in
  the submodule and reports the submodule commit hash.
- Parent pointer diagnostic test: after a submodule response commit, the
  superproject dirty pointer is reported as a separate state, not hidden by a
  generic "committed" message.
- Missing target commit test: visible response patch with dirty target repo
  fails session-check instead of reporting success.
- Recursive fallback test: same-pane recursive dispatch failure cannot be
  closed out as a normal committed response unless the later write/finalize path
  proves commit ownership.

## Non-Goals

- Do not auto-commit arbitrary parent-repo changes.
- Do not stage across repo boundaries unless an explicit parent-pointer commit
  path is requested and validated.
- Do not weaken the recursive direct-invocation deadlock guard.
