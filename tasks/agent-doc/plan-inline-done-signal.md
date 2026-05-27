# Plan: Inline Done Signal Detection

**Bug:** When a user types `done` in exchange, agent-doc has no mechanism to map that to `--done` for a contextually relevant review item. The agent must manually pass `--done <id>`.

## Root Cause

1. `preflight.rs` detects prompt targets but does not interpret `done` as a completion signal.
2. There is no mapping from "inline done signal + nearest review item context" to a `--done` mutation.
3. The `pending_mutations` system in preflight output only carries `expect_add` items, not `expect_done`.

## Proposed Fix

### Phase 1: Prompt signal classification

- Add a new classification step in preflight that identifies user prompts matching patterns: `done`, `mark #id done`, `#id done`, `complete`.
- Classify as `completion_signal` prompt targets.

### Phase 2: Contextual target resolution

- When a `completion_signal` is detected without an explicit `#id`, resolve to the nearest gated review item in context (the review item being discussed in the surrounding exchange).
- When an explicit `#id` is present, use that.

### Phase 3: Mutation emission

- Wire `completion_signal` into the preflight pending mutations as `expect_done` entries.
- `finalize` then includes `--done <id>` automatically.

### Phase 4: Guard integration

- If `review_done_guard` is active, the auto-done path first gates the item (if in backlog), then marks done.
- If `auto_done` is false (default), the signal is noted but not auto-applied; `pending_done_guard` warns as it does today.

## Files to Change

| File | Change |
|------|--------|
| `src/agent-doc/src/preflight.rs` | Add `completion_signal` detection and `expect_done` mutation emission |
| `src/agent-doc/src/session_check.rs` | Wire `completion_signal` into missing-done detection |
| `src/agent-doc/src/write.rs` | Accept `expect_done` mutations from preflight |
| `src/agent-doc/SPEC.md` | Document inline done syntax |

## Testing

- Unit test: `done` in exchange → resolves to nearest gated review item
- Unit test: `done #id` → resolves to explicit id
- Unit test: `auto_done: false` → signal detected but not auto-applied
- Integration test: full cycle with inline done → item auto-marked done in review
