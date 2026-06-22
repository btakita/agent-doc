# Plan: queue auto-strike must not treat referenced ids as resolved directive targets

**Backlog id:** `#qrefmisstrike`
**Class:** clean-session
**Bug class:** preflight queue maintenance / auto-strike
**Related:** `#queueadvgated`, `#qmisstrike`, `#free-text-queue-owner-consume`, `#nextswallow`

## Problem

Live EFS repro, 2026-06-15:

```md
<!-- agent:queue preset="#spec-test-commit-push" priority go -->
- What are #next-steps to follow-up on #gq5c
<!-- /agent:queue -->
```

The user intended a new free-text follow-up prompt. Preflight instead auto-pinned
the queue item, then auto-struck it as already resolved:

```text
[preflight] queue: auto-pinned manually-added prompt(s) with operator priority (#7r2s)
[preflight] queue: auto-struck already-resolved head prompt ":pushpin: What are #next-steps to follow-up on #gq5c" source=review_gated
[preflight] queue: cleared drained queue body
```

The following cycle then did a no-op against the compact archive summary rather
than answering the user's `#next-steps` question. The queue item was deleted even
though no response answered it.

## Root-Cause Hypothesis

`queue_prompt_done_id(text)` extracts the first `#id` substring anywhere in the
queue prompt. That is correct for directive-shaped queue heads such as
`do [#id]`, `do #id`, or a bare `[#id]`, but it is too broad for natural-language
free-text prompts.

For `What are #next-steps to follow-up on #gq5c`, the extractor can treat a
referenced id or preset token as the queue head's completion target. If that id
appears in `agent:done` or `agent:review` `[/]`, preflight converts the whole
question into a completed queue entry and drains the queue.

## Fix Plan

1. Replace broad `queue_prompt_done_id` extraction with an explicit
   `queue_prompt_resolved_target_id` parser.
2. Only return an id for directive-shaped resolved-work queue heads:
   `do [#id]`, `do #id`, `advance [#id]`, `advance #id`, and a bare `[#id]`
   item, after stripping supported queue pin prefixes like `:pushpin:` and
   `:round_pushpin:`.
3. Do not return an id for natural-language prompts that merely reference an id,
   especially prompts containing `#next-steps`, `follow-up on #id`, or multiple
   hashtags where the id is not the directive target.
4. Update telemetry wording from "head prompt" if the implementation still
   strikes resolved prompts anywhere in the queue; the log should make clear
   that only directive target ids are eligible.
5. Update `specs/02-document-format.md` to state that resolved-id auto-strike is
   only for directive-shaped queue entries, not arbitrary free-text references.

## Tests

1. Regression: with `gq5c` in `agent:done` or gated `agent:review`, preflight
   preserves `- What are #next-steps to follow-up on #gq5c` as a live queue
   prompt and does not clear the queue.
2. Pinned regression: preflight preserves
   `- :pushpin: What are #next-steps to follow-up on #gq5c`.
3. Positive regression: `- do [#gq5c]`, `- do #gq5c`, and `- [#gq5c]` still
   auto-strike when `gq5c` is resolved.
4. Multi-hashtag regression: `- Compare #next-steps with #gq5c` does not
   auto-strike even when either id is resolved.
5. End-to-end preflight fixture using the compacted EFS document shape proves
   the queue item remains live and `queue_continuation_required` is true instead
   of producing a no-op closeout.

## Non-Goals

- Do not remove the `agent:review` `[/]` auto-advance behavior for explicit
  `do [#id]` queue heads.
- Do not broaden free-text heading-match queue consumption in finalize.
- Do not implement the EFS `#next-steps` answer as part of this bug fix.
