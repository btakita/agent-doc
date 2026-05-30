# Plan - free-text auto-queue heads must advance in owner-pane closeout

## Problem

Live repro: `src/boost-client/tasks/monsterrodholders.md` queued the free-text head
`Approve [#shoptiers]. What are #next-steps?`. A response was present in the
document, but the queue head stayed live and the next queue item reported that it
"was not marked as done in the queue."

The same session also exposed the owner-pane path: running
`agent-doc <FILE>` inside the Codex pane that already owns the document correctly
refused recursive dispatch, but a manual `agent-doc finalize --force-disk`
response then logged:

```text
[queue] skipped consumption because the active prompt did not target the queue head
```

This leaves `agent:queue auto` stuck even though the free-text queue prompt was
answered.

## Root-Cause Hypothesis

Queue consumption is guarded by proof that the active prompt targeted the current
queue head. The normal `run` path can synthesize that proof from an active queue
head, but owner-pane recursion avoidance and manual repair/write paths can lose
the synthetic queue-target context. Free-text heads have no `#id`, so they cannot
be recovered via `--done <id>` and rely on the "answered free-text queue head"
completion path.

## Recommended Fix

1. When `run` detects same-pane recursive direct invocation with an active ready
   queue head, do not abandon into a state that requires manual response
   patchback. Either:
   - hand the synthetic queue-head prompt back to the owning harness as an
     in-process continuation instruction, or
   - record a durable queue-target capture/cycle marker that a subsequent
     `finalize` can use to permit free-text queue consumption.
2. Extend `finalize` / `write --commit` repair semantics so an exact response
   heading for a free-text queue head, or a captured response during a documented
   recursive-direct recovery, can consume that free-text head.
3. Keep `do [#id]` safety unchanged: id-backed queue heads still require
   explicit `--done`, `--pending-gate`, `--pending-edit`, or the existing exact
   supported completion signal.
4. Improve diagnostics: when a response heading exactly matches the free-text
   head but consumption is skipped, print the missing proof source and the
   supported repair command instead of a generic "did not target" line.

## Tests

1. Regression: active `agent:queue auto` with a free-text first prompt, same-pane
   recursive invocation blocked, then repair/finalize with
   `### Re: <exact queue text>` consumes the head and leaves the second prompt as
   the continuation.
2. Negative regression: `do [#id]` queue head plus a halt/refusal response
   without `--done` does not consume.
3. Snapshot parity regression: document queue and snapshot queue both advance,
   and `session-check --codex-final-gate` reports the next prompt rather than the
   already-answered one.
4. Diagnostic regression: skipped free-text consumption names the missing
   queue-target proof and suggests the new repair path.

## Non-Goals

- Do not reintroduce broad heading-match consumption for id-backed queue heads.
- Do not make manual file patching a supported closeout path.
