---
agent_doc_session: 92fba2ac-9d1f-4a7a-8764-821a8a08ecd0
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

<!-- agent:exchange -->

### Re: judith-krug.md response deleted while reading

**What happened:** The snapshot has a NEWER version ("Re: Attorney handling MA contract") than HEAD ("Re: Mortgage Automator cancellation experiences & sentiment"). This means:

1. The "cancellation experiences" response was written and committed (8276d98)
2. A subsequent agent-doc cycle wrote a new response ("Attorney handling") — replacing the exchange content and updating the snapshot
3. The replacement happened while you were reading the previous response

This is template/replace mode working as designed — each cycle replaces the exchange component. But the UX is jarring when you're actively reading a response that gets replaced under you.

**The current file matches HEAD** (8276d98 — "cancellation experiences"). The "Attorney handling" response exists only in the snapshot. It was written to the file but never committed, and the file appears to have been reverted (possibly by your editor reloading from disk).

**Fixes for this UX problem:**

1. **Don't replace while user is reading:** The watch daemon could detect if the user's cursor is inside the exchange component (via IDE plugin) and defer the write
2. **Append instead of replace:** Change the default component mode for exchange to `append` instead of `replace` — old responses stay visible
3. **Git gutter as guard:** Since the response stays uncommitted until the next cycle, the user sees green gutter bars. But if a new cycle starts before they finish reading, the commit + replace happens too fast

Which approach appeals to you? Or is the root issue that the watch daemon triggered too aggressively?

I like append instead of replace.
I want to use append mode until the entire inquiry is complete. Then we can replace.
There should be a pipeline where we have the exchange component, but also put findings into another component where the data is kept and not removed.
<!-- /agent:exchange -->
