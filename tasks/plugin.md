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

## Assistant

**Fixed.** Double-click guard added to `SubmitAction.kt`.

**Root cause:** No concurrency guard on "Run Agent Doc". Two rapid invocations spawn two `agent-doc route` processes that both `tmux send-keys -l` to the same pane. The second hits while Claude Code's TUI is processing the first command — tmux rejects each of the 11 characters in `/agent-doc ` with "not in a mode".

**Fix:** `AtomicBoolean` guard in `SubmitAction` (same pattern as `EditorTabSyncListener`). While a route is in flight, subsequent clicks show "Route already in progress" hint and skip. The guard releases via `onComplete` callback when the background thread finishes.

**Changes:**
- `SubmitAction.kt` — `AtomicBoolean routing` guard in `actionPerformed()`
- `TerminalUtil.kt` — `sendToTerminal()` accepts optional `onComplete` callback, invoked in `finally` block

**Plugin rebuilt** (both zips). Simple reinstall + restart should suffice.

## User

Is agent-doc-jetbrains and agent-doc-vscode still being used?
agent-doc/editors/jetbrains and agent-doc/editors/vscode should be the canonical source code.

agent-doc-jetbrains and agent-doc-vscode should be deleted.
