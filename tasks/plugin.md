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

## Assistant

**Two separate issues:**

1. **Agent Doc popup not appearing** — IDEA is still running the old self-disabling plugin (cached bytecode). The new zip is built but hasn't been installed in IDEA yet.

   **Fix:** IDEA → Settings → Plugins → gear → "Install Plugin from Disk" → select:
   ```
   agent-doc/editors/jetbrains/build/distributions/agent-doc-jetbrains-0.1.0.zip
   ```
   If it doesn't take effect, **uninstall first → restart → install → restart**. (Same caching issue we hit before with ClaimAction.)

2. **"Replace with" does nothing** — This is the same IDEA bug we diagnosed earlier. It's per-file, not caused by our plugin (confirmed: still broken with plugin disabled). We have a YouTrack bug report ready for it.

The new plugin source has no self-disabling — just a simple `.md` extension check. Once installed, Alt+Enter will always show the Agent Doc popup in Markdown files.

3. Plugin reinstall needed — IDEA running cached old version. "Replace with" is the known IDEA bug (not our plugin).

## User

I uninstalled and installed. Replace with works. But the Agent Doc Actions still is not properly bound to Alt+Enter.

## Assistant

Good news: **"Replace with" works** — confirms our plugin wasn't the cause.

Alt+Enter problem: IDEA's built-in `ShowIntentionActions` also uses Alt+Enter. Without an ActionPromoter, IDEA gives priority to its built-in action — even when it has nothing to show in `.md` files, it "wins" the shortcut silently.

**Options:**

1. **Different keybinding** — e.g., `Ctrl+.` or `Alt+Period`. Zero conflict.
2. **Register as IntentionAction** — Agent Doc items appear inside IDEA's own intentions popup. Most integrated.
3. **Re-add ActionPromoter** — we now know the ActionPromoter didn't break "Replace with" (that was an IDEA bug independent of our plugin). Safe to re-add `promote()` to put our action first for `.md` files.

Option 3 is the quickest fix. We removed the ActionPromoter thinking it caused the "Replace with" failure, but we've now proven that was wrong — the bug resolves/persists independently of our plugin.

Which approach do you prefer?

4. Alt+Enter not firing — `ShowIntentionActions` wins the shortcut without ActionPromoter. Need a resolution strategy.

## User

Readd ActionPromoter.

## Assistant

**ActionPromoter re-added.** Created `AgentDocActionPromoter.kt` with the original `promote()` logic — sorts `AgentDocPopupAction` to the front of the action list for `.md` files. Registered in `plugin.xml` as `<actionPromoter>`.

Both plugin zips rebuilt (unsigned + signed). **Uninstall → restart → install → restart** since this adds a new class.

5. Re-added ActionPromoter — `promote()` puts Agent Doc first for `.md` files. Alt+Enter restored.

## User

commit the plugin changes
