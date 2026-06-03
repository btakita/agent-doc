# Plan: OpenCode Post-Turn False Active-Turn Detection

**Bug ID:** `#opencode-post-turn-false-active`
**Date:** 2026-06-03
**Status:** IMPLEMENTED 2026-06-03 (Option A) — `dispatch_blocker_reason` OpenCode branch now uses a bottom-12-lines busy-cue check (`opencode_active_turn_busy`, keyed on `esc to interrupt`) instead of the all-lines non-idle scan. 2 regression tests added (`dispatch_blocker_reason_post_turn_output_with_idle_bottom`, `dispatch_blocker_reason_post_turn_with_active_working_bottom`); existing opencode active/idle tests stay green. `make check` green, both binaries rebuilt. Remaining: live-verify a real OpenCode pane re-dispatches after a completed turn.
**Related:** `#opencode-build-chrome-stall` (fixed), `#opencode-active-turn-reopen-auto-retry-on-idle` (related but different)

## Symptom

JB "Run Agent Doc" stalls after the first agent-doc turn completes in an OpenCode pane. The dispatch-only reopen path fails with:

```
Error: dispatch-only opencode reopen refused to inject into pane %62 for tasks/software/tsift.md because the pane still shows opencode active turn; restore an idle prompt and retry
```

The pane IS idle (Build · MODEL chrome, box art, status bar visible at bottom), but the detector incorrectly classifies it as an active turn.

## Root Cause

`dispatch_blocker_reason` for OpenCode (`harness.rs:324-346`) scans **all lines** of the captured pane output:

```rust
for line in output.lines().map(crate::prompt::strip_ansi) {
    let trimmed = line.trim();
    if trimmed.is_empty() { continue; }
    if self.is_dispatch_ready_prompt_line(trimmed) {
        has_ready_prompt = true;
        continue;
    }
    if !self.is_ignorable_output_line(trimmed) {
        has_non_idle_content = true;
    }
}
if has_non_idle_content && !has_ready_prompt {
    return Some("opencode active turn".to_string());
}
```

After a turn completes, the pane contains:
1. **Completed turn output** in scrollback (bash commands, results, "Thought: 7.6s", "Click to expand") — NOT idle chrome
2. **Idle chrome** at the bottom (Build · MODEL, ╹ box art, `esc interrupt`, `26.6K (13%) ctrl+p commands OpenCode 1.15.13`)

The completed turn output triggers `has_non_idle_content = true`. The OpenCode idle state has no standalone `>` or `›` prompt line, so `has_ready_prompt = false`. Result: false `"opencode active turn"`.

**Contrast with Claude branch** (`harness.rs:348-364`): Claude checks only the **last 8 lines** for `claude_active_turn_busy()`. It does NOT scan all lines, so completed turn output in scrollback doesn't cause false positives.

## Additional ignorable gaps

The bottom status/input bar line is not fully classified as idle chrome:
- `⬝⬝⬝⬝⬝⬝⬝⬝  esc interrupt  ...  26.6K (13%)  ctrl+p commands • OpenCode 1.15.13`
- `esc interrupt` is an idle keybinding hint, not a busy indicator
- The combined line fails the `tab agents && ctrl+p commands` check (missing `tab agents`)
- The box-art prefix + text content may fail `is_opencode_box_art_line`

However, adding these to `is_ignorable_output_line` alone does NOT fix the root cause — the turn output in scrollback will still trigger `has_non_idle_content`. The fundamental issue is the scan-all-lines strategy.

## Fix: Bottom-N-Lines Busy-Cue Detection (Option A — Recommended)

Mirror the Claude branch strategy: check only the **bottom N lines** for a busy cue, instead of scanning all lines for non-idle content.

### Changes in `harness.rs`

1. **New helper: `opencode_active_turn_busy`** — checks the last N lines (suggest N=12, matching OpenCode's typical bottom-chrome height) for a genuine busy indicator:
   - `Working (Ns - esc to interrupt)` — the actual active-turn busy cue
   - Lines that contain `+ Thought:` without a preceding idle chrome bottom pattern
   - Any line matching the existing busy-cue patterns from the test (`dispatch_blocker_reason_detects_opencode_active_turn`)

2. **Rewrite the OpenCode branch of `dispatch_blocker_reason`:**
   ```rust
   if self.binary == "opencode" {
       let recent = output.lines().rev().take(12)
           .map(crate::prompt::strip_ansi)
           .map(|line| line.trim().to_ascii_lowercase())
           .collect::<Vec<_>>();
       if opencode_active_turn_busy(&recent) {
           return Some("opencode active turn".to_string());
       }
       return None;
   }
   ```

3. **Update `is_ignorable_output_line`** (secondary improvement): add `esc interrupt` as an idle chrome pattern for completeness, even though the primary fix makes it unnecessary for the active-turn check.

### Tests

1. **Keep existing test green:** `dispatch_blocker_reason_detects_opencode_active_turn` — the `Working (21s - esc to interrupt)` line must still trigger `"opencode active turn"`
2. **Keep existing test green:** `dispatch_blocker_reason_allows_opencode_idle_build_chrome` — pure idle chrome returns `None`
3. **New regression test:** `dispatch_blocker_reason_post_turn_output_with_idle_bottom` — pane has completed turn output (bash commands, "Thought: 5s", "Click to expand") above idle bottom chrome (Build ·, box art, status bar) → returns `None`
4. **New regression test:** `dispatch_blocker_reason_post_turn_with_active_working_bottom` — pane has completed turn output but the BOTTOM shows `Working (30s - esc to interrupt)` → returns `"opencode active turn"`

### Risk

- **Low.** The change narrows the detection window from all-lines to bottom-N-lines. A genuine active turn will always show the busy indicator at the bottom. False negatives (missing a real active turn) are unlikely because the `Working` indicator is always rendered at the bottom of the active pane.
- **Aligns with Claude strategy.** The Claude branch already uses this bottom-N approach.

## Alternative: Expand Ignorable Lines (Option B — Not Recommended)

Add every possible OpenCode UI line to `is_ignorable_output_line`:
- "Click to expand", "Context", "tokens", "% used", "$0.00 spent", "LSP", "LSPs are disabled", "Modified Files", "documentation review", "esc interrupt", "Z.AI Coding Plan", etc.

**Rejected because:**
1. Fragile — every OpenCode UI change requires updating the list
2. Doesn't address the fundamental issue of scanning scrollback
3. The turn output itself (bash commands, file contents) can never be exhaustively classified as chrome

## Verification

1. `cargo test -p agent-doc-orchestration -- dispatch_blocker_reason` — all existing + new tests pass
2. `make check` — clean
3. Live verify: run an agent-doc cycle in an OpenCode pane, let it complete, then trigger JB Run Agent Doc again — should dispatch without the "active turn" refusal
