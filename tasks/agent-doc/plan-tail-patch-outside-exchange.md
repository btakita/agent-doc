# Plan: Tail Patch Outside Exchange

**Pending:** `#tailpatch` in `tasks/agent-doc/agent-doc-bugs2.md`
**Status:** Draft

## Problem

A prompt/response pair can be patched into the wrong location in a structured session document: after the live `agent:exchange` block, after parked sections like `# Session Digest`, or effectively at end-of-file. That leaves the content visible but outside the canonical conversation surface.

The `claudescore-3.md` example showed this exact shape:

- the live `agent:exchange` block had already closed above `## Pending / Not Built`
- a later `❯ Why were the backlog items removed without being completed?` prompt and its `### Re:` response were appended after `# Session Digest`
- this was not the intended exchange location and should have failed closed instead of silently landing there

The same failure class was reported again on 2026-05-04:

- the user reported that the last response was written outside `agent:exchange`
- the live template still had an `agent:exchange` component, so the write path had a valid target and nevertheless escaped it
- treat this as evidence that at least one current closeout or repair path still bypasses the structural exchange-boundary invariant

The 2026-05-05 `tasks/agent-doc/agent-doc-bugs2.md` cycle adds newer evidence:

- the session now contains a follow-up `do [#tailpatch]` specifically because the immediately previous response was observed outside the live exchange block
- the current tree already has `template::guard_no_conversation_tail_outside_exchange(...)` plus `template::repair_duplicate_exchange_close_tail(...)`, so this is no longer a purely missing-guard bug; at least one live closeout/repair path still reaches a bad tail shape before those protections fail closed or re-home the response
- the same document also showed repeated `commit_already_current` / recovery churn around the malformed tail, which makes the write/finalize/repair boundary itself part of the investigation rather than only the pure merge normalizer

## Goal

Ensure that prompt-bearing follow-up content for structured session docs can only land inside the live `agent:exchange` block, or the write fails with a precise error.

## Desired Behavior

- if `agent:exchange` exists, prompt/response patchback must target that block
- if the computed insertion point is outside that block, the write path must fail closed
- parked/commented sections and tail prose must not be treated as acceptable fallbacks for exchange insertion

## Implementation Steps

- [ ] Trace the placement logic used by the current template closeout surfaces with `agent:exchange`:
  - normal `write`
  - `finalize`
  - repair / resume
  - routed closeout / hook recovery
- [ ] Reconstruct both known reproductions and identify which path wrote outside the exchange block:
  - the earlier `claudescore-3.md` tail-after-`# Session Digest` example
  - the 2026-05-05 `agent-doc-bugs2.md` closeout that left `#tailpatch` as the next prompt after the previous response escaped the intended exchange boundary
- [ ] Trace why the existing structure guard and duplicate-close repair did not already fail closed or canonicalize the malformed tail before commit:
  - `write.rs` normalization / final-content merge
  - `template.rs` guard + duplicate-close repair
  - `repair.rs` recovery path
  - any `preflight`-driven closeout or adoption path that commits normalized content
- [ ] Tighten the structural invariant: prompt-bearing exchange additions must stay between `<!-- agent:exchange ... -->` and `<!-- /agent:exchange -->` on every response-bearing write path, not just after best-effort normalization
- [ ] Add or strengthen the fail-closed guard so a response-bearing patchback outside that range is rejected before commit rather than only repaired opportunistically afterward
- [ ] Decide whether repair may re-home already-present misplaced exchange content or only report it, and make that rule consistent across `write`, `finalize`, and explicit `repair`
- [ ] Add regressions for:
  - normal append inside `agent:exchange`
  - duplicate `<!-- /agent:exchange -->` tail with a stranded response and boundary marker
  - attempted append after `# Session Digest`
  - attempted append after `agent:backlog`
  - response-bearing closeout that would otherwise commit backlog/other mutations while the exchange response is still outside the live exchange block
  - missing exchange block behavior
- [ ] Update command/spec docs once behavior is settled

## Files Likely To Change

- `src/agent-doc/src/write.rs`
- `src/agent-doc/src/repair.rs`
- `src/agent-doc/src/template.rs`
- `src/agent-doc/src/preflight.rs`
- `src/agent-doc/tests/finalize_integration.rs`
- `src/agent-doc/tests/run_integration.rs`
- `src/agent-doc/specs/07-closeout-commands.md`
- `src/agent-doc/README.md`

## Exit Criteria

- prompt/response patchback cannot silently land after the structured exchange surface
- `claudescore-3.md` style tail placement is either blocked or explicitly recoverable
- the `agent-doc-bugs2.md` reproduction shape fails closed or is deterministically repaired before commit instead of surviving as a committed malformed tail
