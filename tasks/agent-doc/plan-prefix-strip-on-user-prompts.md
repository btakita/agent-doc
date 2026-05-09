# Plan: `❯ ` prefix stripped from user prompts during finalize

## Problem

During the finalize cycle on buildparty-investor-demo/dev.md (commit `ed02213c`, 2026-05-07 13:02), multi-line user prompts were committed without the required `❯ ` prefix while a single-line prompt on the same turn got it correctly.

### Evidence

Committed state at `ed02213c`, lines 113-119:
```
Please increment version to v0.1.1. Release to github. Create a plan for...  ← NO ❯
Miguel will be integrating the demo into                                      ← NO ❯

Please rename the gh repo ClaudeScore/buildparty-investor-demo to...          ← NO ❯
Also, please draft slack instructions for robert-ross and miguel-mendez...    ← NO ❯

❯ spec-test-news-commit-push                                                  ← HAS ❯
```

All six lines are user prompts from the same edit. Only the single-line prompt preset (`spec-test-news-commit-push`) received `❯ `. The five multi-line prompt lines did not.

### Two invariant violations

1. **User prompts must always have `❯ ` prefix** — the prefix is the canonical signal that distinguishes user prompts from agent content in the exchange. Stripping it makes the prompt/response boundary ambiguous for future diff cycles.

2. **Previous turn content must be immutable** — finalize should only append new content (the agent response) and the boundary marker. It should never mutate prefix state on content that was already committed in a prior turn. The `content_ours` fallback re-derives prefixes from scratch, which can mutate committed user prompts.

## Root Cause

The `content_ours` prefix derivation uses line-level heuristics to decide which lines get `❯ `. For multi-line user prompts that span several lines, the heuristic may only match the first or last line of a logical prompt block, or may fail to recognize continuation lines as part of a user prompt.

This is the inverse of `#pfxleak` (which adds `❯ ` to agent content). Both bugs share the same root cause: `content_ours` doesn't preserve prefix state from HEAD. It re-derives from scratch and gets classification wrong.

### Related bugs

- `#pfxleak` — `❯ ` prefix ADDED to agent content during content_ours fallback (plan: plan-content-ours-prefix-and-commit-churn.md)
- `#commitchurn` — excessive commits per turn from session-check re-resolution (same plan)

## Proposed Fix

Extend Phase 1 of the existing plan (`plan-content-ours-prefix-and-commit-churn.md`):

### 1. Preserve prefix state from HEAD (bidirectional)

**File:** `src/agent-doc/src/write.rs` (content_ours fallback)

The existing Phase 1 fix direction says: "Lines committed without `❯ ` in HEAD should never gain `❯ `." This must be extended bidirectionally:

- Lines committed WITH `❯ ` in HEAD must keep `❯ ` (fixes this bug)
- Lines committed WITHOUT `❯ ` in HEAD must not gain `❯ ` (fixes #pfxleak)
- Only NEWLY ADDED lines (not in HEAD) should have prefix derived from heuristics

### 2. Multi-line prompt recognition

**File:** `src/agent-doc/src/write.rs` or `src/agent-doc/src/diff.rs`

When deriving prefixes for new lines, the heuristic must recognize multi-line user prompts. A block of consecutive new lines between a boundary marker and the next `### Re:` heading is a single logical prompt — all lines in the block must get `❯ `.

### 3. Test cases

1. Multi-line user prompt added in a single edit → all lines get `❯ ` after finalize
2. Previously committed user prompt with `❯ ` → prefix preserved through content_ours fallback
3. Previously committed agent content without `❯ ` → no prefix added through content_ours fallback
4. Mixed block: multi-line prompt followed by single-line prompt preset → both get `❯ `
