# Plan: Review Auto-Done

**Bug:** When a user types `done` or indicates completion in exchange, gated review items (`[/]`) are not automatically marked as done. The agent must manually pass `--done <id>` for each item. The `pending_done_guard` detects *missing* done declarations after the fact but does not auto-resolve them.

## Root Cause

1. `session_check::detect_missing_pending_done_ids` (session_check.rs:1219) identifies completed items in prose but only reports them as warnings/errors — it never auto-applies `--done`.
2. The `pending_done_guard` defaults to `Warn`/`Strict` for backlog items but `review_done_guard` defaults to `Off`, so review items have no enforcement at all unless explicitly configured.
3. There is no preflight or finalize hook that inspects the assistant response for `done` or completion signals targeting a specific `#id` and auto-applies `--done`.

## Proposed Fix

### Phase 1: `detect_missing_pending_done_ids` auto-resolve mode

- Add a new frontmatter/project-config option: `auto_done: bool` (default: `false`).
- When `auto_done` is `true` and `detect_missing_pending_done_ids` finds items that the response clearly completes, instead of emitting a warning, preflight writes the `--done` mutations into the finalize command.
- This respects existing guard workflow: if the item is in `agent:backlog` and `review_done_guard` is active, auto-done first gates it into review, then marks done.

### Phase 2: Inline `done` signal detection

- Extend `preflight.rs` to scan new user prompts for patterns like `❯ done`, `❯ mark #id done`, or `❯ #id done`.
- Map these to `--done <id>` mutations in the finalize command automatically.
- The existing `prompt_bearing_changes` already detects these as `prompt_target` entries; wire them into the pending mutation system.

### Phase 3: Session-check repair path

- When `session-check` runs after closeout and finds missing `--done` entries, instead of just reporting `INTERRUPTED`, offer a repair command: `agent-doc write --done <id> --commit <FILE>`.

## Files to Change

| File | Change |
|------|--------|
| `src/agent-doc/src/frontmatter.rs` | Add `auto_done` field |
| `src/agent-doc/src/project_config.rs` | Add `auto_done` config option |
| `src/agent-doc/src/session_check.rs` | `detect_missing_pending_done_ids` → optionally auto-resolve |
| `src/agent-doc/src/preflight.rs` | Scan user prompts for `done` signals, emit pending mutations |
| `src/agent-doc/src/write.rs` | Support auto-done mutation injection from preflight |
| `src/agent-doc/SPEC.md` | Document `auto_done` option and inline done syntax |

## Testing

- Unit tests for `detect_missing_pending_done_ids` auto-resolve path
- Integration test: user types `❯ done` targeting a gated review item → item auto-marked done
- Integration test: `auto_done: false` (default) preserves current behavior
- Session-check test: missing done → repair command suggested
