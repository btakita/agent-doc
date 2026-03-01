---
session: d4eed435-cbf3-44ae-a96c-048b0b2e8025
resume: null
agent: null
model: null
branch: null
---

## JetBrains Plugin — Resolved Issues

### Alt+Enter Keybinding
- **ActionPromoter required** for Alt+Enter — `promote()` puts Agent Doc first for `.md` files
- **"Replace with" is an IDEA bug** (per-file spellcheck issue, not our plugin)
- **Plugin caching** — structural changes require uninstall → restart → install → restart
- Commits: submodule `ccf03d7`, parent `59d702b`

### PromptPoller Race Condition
- **Root cause:** `currentPromptKey`, `answeredPromptKey`, `activePromptQueue` modified by EDT, read by executor thread — stale values caused re-show after answering
- **Fix:** `@Volatile` on all 3 cross-thread fields. `answeredPromptKey` grace period filters only the specific answered prompt (no blanket cooldown)
- Commits: submodule `46d617f` (initial), `f1e71f1` (removed cooldown per user feedback)

### Layout Non-Session Pane Swap
- **Root cause:** `layout.rs` broke out ANY unwanted pane, including non-Claude shells
- **Fix:** Load session registry, only break out registered session panes. Non-session panes (shells, tools) left untouched.
- Commit: submodule `46bf84c`

### PromptPanel Button Overflow
- **Root cause:** Long option text (100+ chars) pushed buttons off-screen — only [3] No visible
- **Fix:** `maxLabelLen` 80 → 45 chars (full text in tooltip), panel height capped at 30% of window
- Commit: submodule `5ab29be`

All pushed to `main` on origin.

## User

I ran Run Agent Doc twice on a document and got an error;

agent-doc route failed (exit 1): not in a mode not in a mode not in a mode not in a mode not in a mode not in a mode not in a mode not in a mode not in a mode not in a mode not in a mode Error: tmux send-keys failed (text)
