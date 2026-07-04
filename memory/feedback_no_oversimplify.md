---
name: Don't oversimplify when asked for "simpler"
description: User's "simpler" means tighter signal/noise, not more verbose explanation. Elaborate on action quotes specifically, not surrounding context.
type: feedback
originSessionId: 6baec669-f140-4511-bf7e-a8b4d25703ca
---
When the user asks for "simpler" or "less jargon", do NOT pad with verbose plain-language reframings. The user prefers the original terse, opinionated tone with recommendations.

**Why:** A previous "simpler" rewrite of the rescore-decision options had lower signal/noise — it explained the surrounding context at length but left the actual action quotes ("just redo it.", "wipe the slate clean first.", "fix the bug before spending more tokens.") unexplained. The user wanted *those quotes* elaborated — what concrete steps each action entails — not the framing around them.

**How to apply:**
- Default to the original tone (terse, opinionated, recommendation included).
- When user requests "simpler"/"less jargon": tighten word count AND expand on the load-bearing action phrases. Make the quotes themselves precise about what will be done.
- Keep recommendations explicit — the user likes having a pick with reasoning.
- If unsure whether to elaborate or compress, ask "which part needs more detail" rather than rewriting wholesale.
- The user will request elaboration explicitly when needed; don't preemptively expand.
