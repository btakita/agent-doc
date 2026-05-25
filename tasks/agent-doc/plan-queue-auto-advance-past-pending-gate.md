# Plan: `agent:queue` auto-advance should also skip `--pending-gate`d items

**Bug class:** preflight / queue auto-advance
**Related:** [#queueautoadvance] (already-landed: strike queue head whose `[#id]` is in `agent:done`)
**Severity:** medium — queue stalls whenever a do-item lands a contained phase but the larger plan still has open phases, forcing the user to manually re-edit the queue

## Problem

`preflight::run_queue_maintenance` (`src/agent-doc/src/preflight.rs:2251`) auto-strikes a queue-head prompt only when its `[#id]` is present in `agent:done`. Items in `agent:review` marked `[/]` (pending-gate, work-in-progress with at least one phase landed) are NOT auto-struck.

Result: when the agent lands a contained phase of a multi-phase plan and `--pending-gate`s the parent item (because remaining phases are not done), the queue head never advances. The user sees a stalled queue and must either re-edit the queue manually or re-prompt the agent to mark items `--done` (defeating the partial-progress signal in `agent:review`).

Direct evidence: this session stalled the queue head at `do [#crdtrespfx]` after Phase 2 landed and the item was `--pending-gate`d; the user explicitly asked "Why is the queue stalling?" and the workaround was to close `--done` and lose the multi-phase tracking.

## Goal

`preflight::run_queue_maintenance` should also auto-strike a queue-head prompt whose `[#id]` is present in `agent:review` with the pending-gate marker `[/]` (or any explicit "in progress with at least one phase committed" signal).

## Plan

### Phase 1: Identify gated IDs from `agent:review` [done]
### Phase 2: Extend `strike_done_queue_head_prompts` [done]
### Phase 3: Tests + spec [done]

**Implementation (landed):** new `collect_agent_review_gated_ids(content)` in `src/preflight.rs` scans the `agent:review` component for items starting with `- [/]` and extracts the `[#id]` value (alphanumeric / `-` / `_` only, lower-cased). The `run_queue_maintenance` call site unions the gated IDs with the existing `agent:done` IDs into a single `eligible_ids` set before calling `strike_done_queue_head_prompts` — same strike contract, broader strike eligibility. Per-prompt telemetry now logs `source=done` vs `source=review_gated` so audit can see which path advanced the queue. SPEC.md updated at `specs/02-document-format.md` with the union semantics under the queue auto-advance section.

**Tests:**
- `collect_agent_review_gated_ids_extracts_only_gated_marker` — `[/]` collected, `[x]`/`[ ]` not.
- `collect_agent_review_gated_ids_returns_empty_when_no_review_component` — no review → empty.
- `collect_agent_review_gated_ids_ignores_backlog_open_items` — backlog `[ ]` never matches.
- `strike_done_queue_head_prompts_strikes_review_gated_items` — gated id in eligible set strikes the head, live head stays intact.
- Existing tests `strike_done_queue_head_prompts_marks_done_items_completed` and `..._returns_none_when_head_is_live` continue to pass.

### Phase 1: Identify gated IDs from `agent:review` (notes — superseded by landed phase above)

**File:** `src/agent-doc/src/preflight.rs`

Add `collect_agent_review_gated_ids(file)` alongside `collect_agent_done_ids_with_root`. The helper parses `agent:review` and returns the set of `[#id]` values for items beginning with `- [/]`. Treat `- [x]` in `agent:review` the same as `agent:done` (already auto-struck).

### Phase 2: Extend `strike_done_queue_head_prompts`

**File:** `src/agent-doc/src/preflight.rs`

Either:
- a) Add a parallel `strike_gated_queue_head_prompts` that walks `agent:review` gated IDs and strikes the queue head when matched, OR
- b) Generalize `strike_done_queue_head_prompts` to take a union of "completed" and "gated" ID sets.

Prefer (b) — single pass, clear contract. The function should distinguish strike reasons in the ops_log line (`source=done` vs `source=review_gated`) so audit can see which path advanced the queue.

### Phase 3: Tests + spec

**File:** `src/agent-doc/src/preflight.rs` (tests), `SPEC.md`

- New unit tests `strike_done_queue_head_prompts_strikes_review_gated_items` and `strike_done_queue_head_prompts_leaves_backlog_items_alone`.
- Update `strike_done_queue_head_prompts_returns_none_when_head_is_live` if needed.
- SPEC.md: document the union semantics under the queue auto-advance section.

### Phase 4: Documentation

Update `tasks/agent-doc/plan-jb-restart-supervisor-busy-refusal.md`-style plans / runbooks to note that `--pending-gate`d items still auto-advance the queue (so authors don't have to choose between losing phase tracking and stalling the queue).

## Testing

- New preflight unit tests pass.
- `make check` clean.
- Manual: queue `do [#X]` while `[#X]` is in `agent:review` with `- [/]`, run preflight, queue head strikes through.
- Regression: `do [#Y]` where `[#Y]` is in `agent:backlog` (`- [ ]`) — queue must NOT auto-strike (backlog is unstarted work).
