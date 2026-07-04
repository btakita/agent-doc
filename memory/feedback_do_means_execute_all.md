---
name: "do" means execute all listed items now
description: When user writes "do" with a task list, execute ALL items in the list. Don't selectively execute some and defer others based on prior annotations.
type: feedback
---

When the user writes "do" followed by a task list, execute ALL items in that list NOW. Don't selectively execute some and defer others.

**Why:** The user wrote "do" with 4 items. I executed the first (SPEC.md) then jumped to release, ignoring the other 3 because they were previously annotated "(separate session)". But the user's "do" command overrides those annotations — it's a direct instruction to execute everything listed.

**How to apply:** "do" = execute all listed items. Prior annotations about timing ("separate session", "when ready", "deferred") are superseded by the explicit "do" instruction. Don't cherry-pick items from a "do" list.
