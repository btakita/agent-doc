# Spec: Pending Capture Guard

**Status:** Implemented
**Date:** 2026-04-24
**Relates to:** `session-check`, `finalize`, `write --commit`, `pending_cmd`

---

## Problem

LLM agents routinely generate responses containing actionable recommendations — numbered lists, priority-labeled items (P0/P1/P2), explicit "next steps" — but fail to add them to the document's `agent:pending` component. The pending capture rule in the skill/prompt contract says to track these, but agents forget. The result is actionable work buried in exchange prose, invisible to future cycles.

**Observed failure:** In `tasks/claudescore-3.md`, the agent produced a report with 7 prioritized recommendations (3 P0, 2 P1, 2 P2) and committed the response without any `--pending-add` flags. The user had to catch and correct this manually.

This is a systemic pattern, not a one-off. The agent has the right instructions but no enforcement mechanism.

---

## Design

### Where: `session-check` (post-commit validation)

Add the guard to `session-check` rather than `finalize` for three reasons:

1. **Separation of concerns.** `finalize` owns the write+commit pipeline. Adding content analysis there mixes I/O with heuristics.
2. **Self-correction window.** When `session-check` warns, the agent can still add pending items via a follow-up `agent-doc write --commit` with `--pending-add` flags in the same turn — the skill contract already allows this recovery pattern.
3. **Catch-all coverage.** `session-check` runs after both `finalize` and manual `write --commit` paths, so the guard covers all response persistence routes.

### What: Heuristic detection of uncaptured recommendations

Scan the **committed response capture** for patterns that indicate actionable recommendations. In template mode, prefer `exchange` / `findings` patch content from the captured response body; otherwise fall back to the unmatched/raw response text. Compare that against whether the cycle recorded any pending-add mutations.

#### Detection patterns (ordered by signal strength)

1. **Priority labels:** Lines matching `P[0-3]:`, `[P0]`, `[P1]`, `Priority 0`, etc.
2. **Numbered action lists:** Ordered lists (`1.`, `2.`, `3.`+) where items contain action verbs (`add`, `update`, `fix`, `implement`, `create`, `remove`, `migrate`, `refactor`, `ship`, `land`, `merge`)
3. **Recommendation headers:** Headings or bold text containing "Recommendation", "Next Steps", "Action Items", "TODO", "Follow-up"
4. **Imperative sentences after recommendation context:** Lines starting with imperative verbs within 5 lines of a "recommend" keyword

#### What counts as "captured"

A recommendation is considered captured if **any** `--pending-add` flag was provided to the `finalize` or `write --commit` command in the same cycle. The guard does not attempt to match specific recommendations to specific pending items — it checks for the presence of the pending-add intent, not 1:1 coverage.

Rationale: matching individual items to pending entries requires semantic understanding that belongs in the LLM, not in a binary heuristic. The binary's job is to catch the "forgot entirely" case.

### Output

#### Warning mode (default)

When uncaptured recommendations are detected:

```
[session-check] warn: response contains ~N recommendation-like items but no --pending-add flags were used this cycle
[session-check] hint: consider adding pending items for actionable follow-up work
```

Exit code: **0** (pass). The warning goes to stderr so agents see it and can self-correct, but the cycle is not blocked.

#### Strict mode (opt-in)

When `pending_capture_guard = "strict"` is set in document frontmatter or `.agent-doc/config.toml`:

```
[session-check] error: response contains ~N recommendation-like items but no --pending-add flags were used this cycle
[session-check] hint: re-run with --pending-add flags or set pending_capture_guard = "warn" to downgrade
```

Exit code: **1** (fail). The agent must add pending items before the turn can complete.

#### Suppression

- `pending_capture_guard = "off"` in frontmatter or config disables the check entirely.
- If the response contains `<!-- no-pending-capture -->` anywhere, the guard is suppressed for that cycle. This handles cases where the agent explicitly decided not to promote recommendations (e.g., mutually exclusive options presented for user choice).

### Configuration precedence

1. Document frontmatter `pending_capture_guard:` (highest)
2. `.agent-doc/config.toml` `[guards] pending_capture = "warn"` 
3. Default: `"warn"`

---

## Implementation

### Files to modify

| File | Change |
|------|--------|
| `src/session_check.rs` | Add `check_pending_capture()` after existing cycle/bypass checks. Load the committed response capture, run heuristics, check cycle state for pending mutations, emit warning/error. |
| `src/cycle_state.rs` | Add `had_pending_mutations: bool` field to cycle state, set to `true` when any `--pending-add` flag is processed during `write`/`finalize`. |
| `src/write.rs` → `run_command()` | Set `had_pending_mutations = true` on the cycle state when pending-add flags are present (around line 285). |
| `src/frontmatter.rs` | Add `pending_capture_guard` field to document frontmatter parsing. Values: `"warn"` (default), `"strict"`, `"off"`. |
| `src/project_config.rs` | Add project-level `[guards] pending_capture = "warn|strict|off"` parsing for `.agent-doc/config.toml`. |
| `src/heuristics.rs` (new) | Recommendation detection heuristics. Exported as `detect_uncaptured_recommendations(response_text: &str) -> RecommendationSignal`. Keep heuristics isolated for testability. |

### Heuristic module (`src/heuristics.rs`)

```rust
pub struct RecommendationSignal {
    pub estimated_count: usize,
    pub patterns_matched: Vec<PatternKind>,
    pub confidence: f32, // 0.0–1.0
}

pub enum PatternKind {
    PriorityLabel,
    NumberedActionList,
    RecommendationHeader,
    ImperativeAfterRecommend,
}

pub fn detect_uncaptured_recommendations(text: &str) -> RecommendationSignal;
```

The function scans line-by-line, tracks context (are we inside a numbered list? near a "recommend" keyword?), and returns the aggregate signal. The `estimated_count` is the number of distinct items detected, not a precise count.

**Threshold:** Only warn when `estimated_count >= 2` AND `confidence >= 0.5`. A single action item is too common and noisy; the guard targets the "forgot a whole batch of recommendations" failure mode.

### Tests shipped

| Test | Description |
|------|-------------|
| `test_no_recommendations_no_warning` | Plain prose response, no action items → no warning |
| `test_numbered_recommendations_no_pending` | Response with "Recommendations: 1. Add X, 2. Fix Y, 3. Update Z" and no pending mutations → warning |
| `test_numbered_recommendations_with_pending` | Same response but cycle had `--pending-add` → no warning |
| `test_priority_labels_detected` | "P0: Fix scorer prompt" patterns detected |
| `test_strict_mode_exits_nonzero` | Strict config + uncaptured recommendations → exit 1 |
| `test_suppression_marker` | Response contains `<!-- no-pending-capture -->` → no warning |
| `test_config_precedence` | Frontmatter overrides config.toml |
| `test_low_confidence_no_warning` | Single vague action item → below threshold, no warning |
| `test_mutually_exclusive_options` | "Option A: ... Option B: ..." with "choose one" → below threshold (options, not recommendations) |

### Spec/doc updates

| File | Change |
|------|--------|
| `specs/07-commands.md` | Add `session-check` pending capture guard documentation |
| `SKILL.md` | Note that `session-check` now warns on uncaptured recommendations |
| `VERSIONS.md` | Record the new guard behavior |

---

## Edge Cases and Non-Goals

**Not in scope:**
- Matching individual recommendations to specific pending items (semantic matching belongs in the LLM)
- Detecting recommendations in non-exchange components (status updates, etc.)
- Retroactive scanning of historical responses (only the current cycle's response is checked)

**Edge cases:**
- Compacted exchanges where the response is a summary → the guard runs on the current cycle's response, not the compacted history
- Empty pending component (document has no `agent:pending`) → the guard still warns, because the agent should have created the component
- Response contains recommendations but also `--pending-done` flags (not `--pending-add`) → still warns, because done != add
