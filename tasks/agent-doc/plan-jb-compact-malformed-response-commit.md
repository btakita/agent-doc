# Plan: JB Compact Exchange can commit malformed response shape

**Bug ID:** `#jb-compact-malformed-response-commit`
**Status:** planned
**Observed:** 2026-05-30 in `src/boost-client/tasks/monsterrodholders.md`

## Problem

JetBrains `Compact Exchange` committed a malformed exchange shape instead of failing
closed or producing a clean compacted summary.

The visible committed state in `src/boost-client` commit `f8fe36b` includes:

- a compact summary line prefixed as if it were a user prompt:
  `❯ *Compacted. Content archived ...*`
- a compacted prior response body still visible under `### Re: #next-steps-2`
  after the compact summary
- the subsequent `### Re: #gads106demote` response appended after that malformed
  compacted block

That violates the compact boundary: compacted content should be archived and the
remaining exchange tail should preserve valid prompt/response structure.

## Desired Behavior

`Compact Exchange` must only commit a document if the post-compact exchange is
structurally valid:

- no compact summary line is prefixed with the user prompt marker
- archived response bodies are removed from the live exchange except for the
  bounded summary metadata
- remaining prompt and `### Re:` blocks still form valid exchange segments
- `session-check` rejects malformed compact output before it reaches `HEAD`

## Plan

1. Add a compact-exchange regression fixture based on the `monsterrodholders.md`
   shape before and after commit `f8fe36b`.
2. Audit the compact summary renderer and prompt-prefix normalization path for
   summary lines that inherit `❯ ` from an archived prompt tail.
3. Add a post-compact structural validation step before commit:
   - parse the exchange component
   - reject `❯ *Compacted...` summary lines
   - reject unarchived response bodies that belong to archived topics
4. Route the failure through a precise repair diagnostic instead of committing a
   malformed exchange.
5. Add `session-check` coverage so a malformed compacted exchange in `HEAD` is
   diagnosed as compact output drift, not generic direct response patchback.

## Likely Files

- `src/agent-doc/src/compact.rs`
- `src/agent-doc/src/template.rs`
- `src/agent-doc/src/session_check.rs`
- `src/agent-doc/tests/finalize_integration.rs`
- JetBrains compact command wrapper if it bypasses any CLI validation path

## Verification

- SimWorld or integration test: malformed compact output is rejected before
  commit.
- Positive compact test: normal compact output still commits cleanly.
- Live replay from JetBrains `Compact Exchange` on a copy of
  `monsterrodholders.md` leaves a valid exchange and archives the compacted body.
