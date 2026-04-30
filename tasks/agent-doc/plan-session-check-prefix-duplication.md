# Plan: Session-check `❯ ` prefix failure causes response duplication

## Problem

When `agent-doc finalize` commits the response successfully but `session-check` fails because the user prompt line lacks the binary-owned `❯ ` prefix, the skill's recovery path (`write --commit`) re-writes the identical response content. The binary does not detect that the response heading already exists in the exchange, so it appends a second copy.

This happened twice in `tasks/claudescore-3.md` on 2026-04-29:
1. `#sbtcconduct` response duplicated (lines 259 and 311)
2. "speeding up benchmark runs" response duplicated (lines 312 and 333)

Both times the sequence was:
1. `finalize` → response written + committed → session-check exits 1 (missing `❯ ` prefix on user prompt)
2. Skill runs `write --commit` with same response → binary appends second copy

## Root Cause

Two independent issues:

1. **`finalize` does not canonicalize user prompt prefixes.** The `❯ ` prefix should be added during the write pipeline when the binary inserts the response after a bare user prompt line. Currently it's only added during boundary repositioning or normalization, which runs after the write.

2. **`write --commit` does not detect duplicate responses.** When called with response content whose heading already exists in the exchange, it should either skip the write or merge instead of blindly appending.

## Proposed Fix

### Phase 1: Canonicalize prompt prefix during finalize write

In `finalize`'s write pipeline (before the response content is merged into the document), scan for bare prompt-target lines that precede the response insertion point and add the `❯ ` prefix. This prevents the session-check failure that triggers the recovery path.

### Phase 2: Duplicate response guard in write

Add a guard in `write --commit` (and `finalize`) that checks whether the response heading (e.g., `### Re: topic — model`) already exists in the exchange after the most recent user prompt. If it does, skip the write and log a warning instead of appending a duplicate.

### Phase 3: Regression test

Add an integration test that:
1. Creates a document with a bare user prompt (no `❯ ` prefix) followed by `<!-- /agent:exchange -->`
2. Runs `finalize` with a response
3. Verifies the prompt gets `❯ ` prefix AND the response appears exactly once
4. Runs `finalize` again with the same response
5. Verifies no duplicate is created
