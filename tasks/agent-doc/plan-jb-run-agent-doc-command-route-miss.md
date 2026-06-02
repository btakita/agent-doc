# Plan: JB Run Agent Doc — OpenCode False Active Turn Route Miss

## Bug

JB `Run Agent Doc` fails with:
```
Error: dispatch-only opencode reopen refused to inject into pane %42 for tasks/software/tsift.md because the pane still shows opencode active turn; restore an idle prompt and retry
```

The pane is actually idle (no turn running), but route falsely detects "opencode active turn".

## Root Cause

`harness.rs:324-345` — `dispatch_blocker_reason` for OpenCode iterates ALL lines of captured pane output:
1. Sets `has_ready_prompt = true` if any line matches `>` or `›` (dispatch-ready prompt)
2. Sets `has_non_idle_content = true` for any line that isn't ignorable chrome
3. If `has_non_idle_content && !has_ready_prompt` → "opencode active turn"

Two failure modes:
- **A. Completed response above prompt:** Previous assistant output lines are non-ignorable content. If the `>` prompt is present, `has_ready_prompt` should be true and the check should pass. But if the OpenCode TUI renders the idle prompt with ANSI escape sequences that don't strip to a bare `>`, or the prompt is scrolled/partial, `has_ready_prompt` stays false while `has_non_idle_content` is true → false active turn.
- **B. No prompt visible:** OpenCode TUI composer uses a full-width input widget, not a bare `>` character. The idle composer may render as a box/input field with no recognizable prompt glyph. The `>`/`›` check at line 209 is too narrow.

## Fix Options

### Option A: Bottom-up prompt detection (Recommended)

Instead of scanning all lines, check from the bottom up. The idle prompt/composer should be the last visible line. If the bottom lines are chrome + a prompt, treat as idle regardless of content above.

1. Reverse-iterate from the bottom of captured output
2. Skip trailing ignorable chrome lines (context % used, cwd/version, tab agents, etc.)
3. If the first non-chrome line from bottom is a dispatch-ready prompt (`>`/`›` or any future OpenCode idle composer pattern), the pane is idle
4. Only if the first non-chrome line from bottom is actual busy content (spinner, progress), classify as active turn

This mirrors the Claude approach (check last 8 lines) and avoids false positives from completed assistant text above the composer.

### Option B: Hook-based bypass

As the user suggested, use tmux to pass `/agent-doc <file>` directly regardless of route's busy detection. This is a bypass, not a fix — it risks injecting into a genuinely busy pane.

Could be implemented as a `--force` flag on `route --dispatch-only` that skips `dispatch_blocker_reason`. Risk: duplicate triggers during live turns.

### Option C: OpenCode-specific idle composer patterns

Expand `is_dispatch_ready_prompt_line` for OpenCode to recognize the full-width input widget / box-art idle composer. Audit the OpenCode TUI rendering to capture all idle-state prompt shapes.

## Recommended: Option A (Bottom-up) + Option C (Expanded patterns)

1. **Refactor `dispatch_blocker_reason` for OpenCode** to use bottom-up scanning:
   - Reverse-iterate lines, skip chrome, check if last non-chrome line is dispatch-ready
   - Non-chrome content ABOVE the prompt should not trigger active turn
   - This handles completed responses sitting above the idle composer

2. **Expand `is_dispatch_ready_prompt_line` for OpenCode:**
   - Audit live OpenCode TUI rendering to capture all idle prompt shapes
   - Add any missing patterns (input widget borders, cursor-only lines, etc.)
   - Add regression tests for each pattern

3. **Add `--force` escape hatch** (Option B subset):
   - `route --dispatch-only --force` skips `dispatch_blocker_reason`
   - JB plugin can offer this as a fallback action
   - Logs a warning when force-dispatch is used

## Files

- `agent-doc-orchestration/src/harness.rs` — `dispatch_blocker_reason` (lines 324-345), `is_dispatch_ready_prompt_line` (line 209), `is_ignorable_output_line` (lines 216-237)
- `agent-doc-orchestration/src/route.rs` — dispatch refusal at lines 2390-2427, `dispatch_active_turn_queue_source` at 2938-2947
- `agent-doc-orchestration/src/flow/routed_reopen.rs` — flow-level dispatch guard

## Regression Tests

1. `dispatch_blocker_reason` returns None when OpenCode pane shows completed assistant text + idle `>` prompt
2. `dispatch_blocker_reason` returns None when OpenCode pane shows only chrome + idle prompt (no assistant text)
3. `dispatch_blocker_reason` returns "opencode active turn" when OpenCode pane shows a real working spinner
4. Bottom-up detection ignores assistant output above the prompt
5. New `is_dispatch_ready_prompt_line` patterns for any discovered OpenCode idle shapes
