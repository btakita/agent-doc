---
session: d4eed435-cbf3-44ae-a96c-048b0b2e8025
resume: null
agent: null
model: null
branch: null
---

## JetBrains Plugin — Alt+Enter Keybinding Fix

**Issue:** Alt+Enter stopped opening Agent Doc Actions popup after adding self-disabling `update()` logic.

**Root cause:** Self-disabling was unnecessary — it was added to work around a "Replace with" spellcheck bug that turned out to be an IDEA bug, not caused by our plugin.

**Key discovery:** After clean reinstall (uninstall → restart → install → restart), "Replace with" worked fine. The ActionPromoter was never the cause of that bug.

### Resolution

- **Removed self-disabling** — `AgentDocPopupAction.update()` now just checks `.md` extension
- **Re-added ActionPromoter** — `AgentDocActionPromoter.kt` with `promote()` to put Agent Doc first for `.md` files (required for Alt+Enter to win over IDEA's `ShowIntentionActions`)
- **Removed `com.intellij.modules.lang` dependency** — no longer needed without HighlightInfo

### Files changed

| File | Change |
|------|--------|
| `AgentDocActionPromoter.kt` | Re-created with `promote()` logic |
| `AgentDocPopupAction.kt` | Simplified — removed HighlightInfo/severity check |
| `plugin.xml` | Re-added `<actionPromoter>` registration |

### Commits

- **Submodule** `ccf03d7` — re-add ActionPromoter, simplify popup action
- **Parent** `59d702b` — updated submodule pointer

### Lessons

1. **ActionPromoter is required** for Alt+Enter — without it, IDEA's `ShowIntentionActions` wins the shortcut silently
2. **"Replace with" is an IDEA bug** — per-file, not caused by our plugin, confirmed by clean reinstall test
3. **Plugin caching** — structural changes (new classes) require uninstall → restart → install → restart
4. **Markdown HighlightInfo** — `.md` files are saturated with INFORMATION-severity highlights for syntax elements; any filter that checks for HighlightInfo presence will always trigger

## User


