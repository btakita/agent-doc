# Plan: Tail Patch Outside Exchange

**Pending:** `#tailpatch` in `tasks/agent-doc/agent-doc-bugs2.md`
**Status:** Draft

## Problem

A prompt/response pair can be patched into the wrong location in a structured session document: after the live `agent:exchange` block, after parked sections like `# Session Digest`, or effectively at end-of-file. That leaves the content visible but outside the canonical conversation surface.

The `claudescore-3.md` example showed this exact shape:

- the live `agent:exchange` block had already closed above `## Pending / Not Built`
- a later `❯ Why were the backlog items removed without being completed?` prompt and its `### Re:` response were appended after `# Session Digest`
- this was not the intended exchange location and should have failed closed instead of silently landing there

## Goal

Ensure that prompt-bearing follow-up content for structured session docs can only land inside the live `agent:exchange` block, or the write fails with a precise error.

## Desired Behavior

- if `agent:exchange` exists, prompt/response patchback must target that block
- if the computed insertion point is outside that block, the write path must fail closed
- parked/commented sections and tail prose must not be treated as acceptable fallbacks for exchange insertion

## Implementation Steps

- [ ] Trace the placement logic used by write/repair/manual patchback flows for template docs with `agent:exchange`
- [ ] Define a structural invariant: prompt-bearing exchange additions must stay between `<!-- agent:exchange ... -->` and `<!-- /agent:exchange -->`
- [ ] Add a guard that rejects patchback placement outside that range
- [ ] Decide whether repair may re-home already-present misplaced exchange content or only report it
- [ ] Add regressions for:
  - normal append inside `agent:exchange`
  - attempted append after `# Session Digest`
  - attempted append after `agent:backlog`
  - missing exchange block behavior
- [ ] Update command/spec docs once behavior is settled

## Files Likely To Change

- `src/agent-doc/src/write.rs`
- `src/agent-doc/src/repair.rs`
- `src/agent-doc/src/diff.rs`
- `src/agent-doc/specs/07-commands.md`
- `src/agent-doc/README.md`

## Exit Criteria

- prompt/response patchback cannot silently land after the structured exchange surface
- `claudescore-3.md` style tail placement is either blocked or explicitly recoverable
