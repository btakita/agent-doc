# Plan - write closeout response shape recovery

**Pending:** `#write-closeout-response-shape`
**Status:** Draft

## Problem

Live repro, 2026-06-02, from a Codex-owned pane on:

`src/boost-client/tasks/monsterrodholders.md`

The assistant completed work and returned a final response, but the response was
not written into the document. A recovery attempt with:

```bash
agent-doc write --commit --force-disk tasks/monsterrodholders.md
```

accepted stdin and committed the response text, but because the stdin did not
contain a normal closeout heading, the exchange ended as:

```text
❯ do [#stevepackets]. #spec-test-commit-push
Completed `#stevepackets`.
```

`session-check` still classified the live tail as an unresolved prompt-only
closeout. Manually inserting a `### Re:` heading made the response visible, but
then `agent-doc write --commit` deduped against the visible content and refused
to commit it as a direct response patchback. The documented recovery command
looped between "empty response", "already present", and "direct patchback"
states until a manual commit plus `agent-doc commit` finished the parent pointer.

## Desired Behavior

Response recovery should be a single reliable operation:

- if stdin contains a closeout body without `### Re:`, synthesize a valid
  response heading or preserve enough metadata for `session-check` to accept it
- if a visible `### Re:` response is already present, `agent-doc write --commit`
  should be able to adopt it through the snapshot/commit boundary instead of
  deduping first and then refusing it as a direct patchback
- recovery diagnostics must not point to a command that immediately fails with
  "empty response" or "response already present"
- nested-repo parent pointer recovery should remain explicit and should not
  mask whether the target document response has been accepted

## Plan

1. Add a fixture for a compacted exchange with a user prompt tail and a closeout
   response body lacking `### Re:`.
2. Reproduce the current `write --commit` behavior:
   - stdin response is appended
   - no template patches are found
   - response text is committed without a response heading
   - `session-check` still reports unresolved prompt-only tail
3. Add a response-normalization step before unmatched-content insertion:
   - if the active tail is a user prompt and stdin has no heading, synthesize a
     conservative `### Re: <prompt summary>` heading
   - ensure this path is deterministic and does not duplicate existing headings
4. Add a second fixture for the visible-direct-patchback recovery state:
   - working tree contains a `### Re:` response after a prompt
   - HEAD/snapshot do not own that response
   - `write --commit` is invoked with matching stdin or no stdin
5. Change recovery adoption order so an already-visible matching response can be
   committed/adopted before dedup short-circuits the write path.
6. Update `session-check` recovery text to name the correct command for each
   shape:
   - no visible response body
   - visible response body but no committed snapshot boundary
   - committed target doc but stale parent submodule pointer
7. Add regressions for all three states and run the existing write/finalize
   suites.

