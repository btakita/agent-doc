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

Just make sure Alt+Enter still opens the intention menu

## Assistant

**Self-disabling removed entirely.** You're right — the "Replace with" bug was an IDEA bug, not our plugin. No reason for the HighlightInfo complexity.

`AgentDocPopupAction.update()` now just checks `.md` extension. Alt+Enter behavior:

- **No intentions at caret** → Agent Doc popup shows directly
- **Intentions at caret** → IDEA shows a disambiguation popup (choose "Agent Doc Actions" or "Show Intention Actions")

Removed `HighlightInfo`/`HighlightSeverity` imports and `com.intellij.modules.lang` dependency.

Both plugin zips rebuilt (unsigned + signed).

2. Removed self-disabling — simple `.md` check only. Alt+Enter always available; IDEA disambiguates when intentions also exist.

## User

Now the Agent-Doc Actions doesn't popup AND Replace me does nothing 

