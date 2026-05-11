# Plan: Empty-response repair write should not fail after prior closeout

## Problem

When a user asks to recover a missed response patchback, `agent-doc <file>` can
detect that the prior response is already committed, decline to synthesize a
second response, and then still submit to Codex with an empty body:

```text
prior response is already committed in HEAD ... no new assistant response body was supplied
Submitting to codex...
Error: codex command failed: Reading prompt from stdin...
```

That leaves the operator with no binary-owned way to apply the known last
response to the document boundary, even though `agent-doc write --commit` is the
documented repair path.

## Goal

Make missed-patchback recovery deterministic:

- if the user supplies a response body, `agent-doc write --commit` applies it to
  the target document and commits the owning repo
- if no response body exists, `agent-doc` fails before Codex dispatch with a
  precise recovery message
- if the previous response is already committed, the command reports that state
  cleanly and never tries an empty Codex handoff

## Implementation Steps

- [ ] Reproduce the `src/session-share/tasks/root.md` May 2026 shape where
      `agent-doc <file>` prints "no new assistant response body was supplied"
      and then exits from an empty Codex handoff.
- [ ] Split the "already committed" classification from the dispatch path so it
      becomes a terminal no-op or a specific repair prompt, not a child-agent
      invocation.
- [ ] Add a repair/write mode test where stdin contains a template
      `<!-- patch:exchange -->` response and `write --commit` applies it to the
      original exchange boundary without rerunning completed side effects.
- [ ] Add an empty-stdin regression that exits nonzero before child-agent
      dispatch and names the expected command:
      `agent-doc write --commit <file>`.
- [ ] Verify template/CRDT documents and submodule-owned session documents both
      commit through the owning repo.

## Exit Criteria

- missed-response repair never depends on an empty child-agent submit
- operator-facing diagnostics distinguish "already committed", "body missing",
  and "body supplied but write failed"
- `session-check` passes after a successful repair write
