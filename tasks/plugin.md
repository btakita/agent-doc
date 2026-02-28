---
session: d4eed435-cbf3-44ae-a96c-048b0b2e8025
resume: null
agent: null
model: null
branch: null
---
I lost the Alt+Enter to open Agent Doc Actions key binding. We made a recent change for Agent Doc Actions to disable itself with certain conditions.

## Assistant

**Fixed — severity filter was too broad.** The self-disabling `update()` checked for ANY `HighlightInfo` at the caret, but Markdown files are saturated with `INFORMATION`-severity highlights for syntax elements (headers, paragraphs, links, emphasis). The caret was almost always inside one, so Agent Doc was permanently disabled.

**Fix:** Added `tooltip.severity > HighlightSeverity.INFORMATION` filter. Now only disables for:
- **TYPO** (spellcheck) — "Replace with" intentions
- **WEAK_WARNING** / **WARNING** / **ERROR** — code inspections

Ignores `INFORMATION`-level syntax decoration that doesn't produce intentions.

Both plugin zips rebuilt (unsigned + signed). Install from disk — no uninstall needed.

1. Alt+Enter severity filter — `HighlightInfo.severity > INFORMATION`. Agent Doc popup restored for Markdown files.

## User

Is the self-disabling necessary? I only had it to fix the Replace with but that was an Idea bug or an issue with another plugin.

Just make sure 
