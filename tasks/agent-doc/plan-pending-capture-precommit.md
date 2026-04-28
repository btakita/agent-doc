# Plan: Pre-commit Pending Capture Gate

**Status:** Proposed
**Date:** 2026-04-27
**Relates to:** `spec-pending-capture-guard.md`, `finalize`, `session-check`

---

## Problem

The pending-capture guard (`session-check`) runs post-commit. When it detects uncaptured recommendations, the response is already committed without pending items. The agent must then do a follow-up `write --commit` with `--pending-add` flags — an extra round-trip that adds complexity and can fail.

Two improvements would catch the problem earlier:

1. **Pre-commit gate in `finalize`**: scan the response content for recommendation patterns _before_ committing. If detected and no `--pending-add` flags are present, block the commit and report the error. The agent then retries `finalize` with the flags.

2. **`agent-doc plan` emitting `pending_mutations`**: when `plan` analyzes the diff and detects that the exchange context suggests actionable follow-up work will be generated, include `pending_mutations` in the planning record. This makes pending items part of the execution contract before the agent even starts composing.

---

## Design

### Phase 1: Pre-commit gate in `finalize`

**Where:** `src/finalize.rs`, before the commit step.

**What:** Reuse `detect_uncaptured_recommendations()` from `src/heuristics.rs` (already implemented for `session-check`). Run it on the response content that's about to be committed. If the signal exceeds threshold and no `--pending-add` flags are on the command, emit an error and exit non-zero without committing.

**Config:** Reuse `pending_capture_guard` setting. When `strict`, the pre-commit gate is active. When `warn`, only the post-commit `session-check` fires (current behavior). When `off`, both are disabled.

**Files to modify:**

| File | Change |
|------|--------|
| `src/finalize.rs` | Add pre-commit check: run `detect_uncaptured_recommendations()` on response content, check for `--pending-add` flags, block commit if strict and uncaptured |
| `src/heuristics.rs` | No changes — reuse existing function |

**Tests:**

| Test | Description |
|------|-------------|
| `test_precommit_blocks_without_pending_add` | Strict mode + recommendations in response + no `--pending-add` → finalize exits 1, no commit |
| `test_precommit_passes_with_pending_add` | Same but with `--pending-add` → commit proceeds |
| `test_precommit_inactive_in_warn_mode` | Warn mode → commit proceeds, post-commit session-check warns |

### Phase 2: `plan` emits `pending_mutations`

**Where:** `src/plan.rs`

**What:** After computing `prompt_targets`, analyze the exchange context for signals that the upcoming response is likely to generate recommendations:
- User asked for "tasks", "todo", "backlog", "what's next", "recommendations"
- Previous responses in the exchange contain numbered lists that were captured to pending (learned pattern)
- The prompt target includes imperative verbs targeting multiple items

When detected, emit `pending_mutations` in the planning record:

```json
{
  "pending_mutations": [
    {"action": "expect_add", "reason": "user requested backlog/recommendations"}
  ]
}
```

The skill contract already says to execute from the planning record. `expect_add` tells the agent that the finalize command must include `--pending-add` flags.

**Files to modify:**

| File | Change |
|------|--------|
| `src/plan.rs` | Add pending-mutation detection heuristic after prompt_target computation |
| `src/plan.rs` | Emit `pending_mutations` field in JSON output |

**Tests:**

| Test | Description |
|------|-------------|
| `test_plan_detects_backlog_request` | User prompt "add to backlog" → `expect_add` in pending_mutations |
| `test_plan_detects_recommendation_request` | User prompt "what should we do next?" → `expect_add` |
| `test_plan_no_false_positive_on_questions` | User prompt "how does X work?" → no pending_mutations |

---

## Sequencing

Phase 1 is independent and higher-value — it catches the failure at commit time with zero false positives (the response is already generated, so heuristics are concrete). Ship first.

Phase 2 is advisory and depends on prompt-level heuristics that may have false positives. Ship after Phase 1 is validated.
