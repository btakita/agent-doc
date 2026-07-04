---
name: No hallucinating under uncertainty
description: Say "I don't know" when uncertain rather than speculating about what happened
type: feedback
---

When debugging and the evidence doesn't clearly explain the behavior, say "I don't know" rather than constructing a plausible-sounding but unverified explanation.

**Why:** User called out speculative reasoning about tmux pane visibility ("thin sliver or out of view") when the real answer was unknown. Fabricated explanations waste time and erode trust.

**How to apply:** After presenting known facts from logs/code, if there's a gap in understanding, either:
1. Say "I don't know why X happened" and suggest what additional data would help
2. Explicitly label speculation: "Speculation: it might be because..." — never present guesses as analysis

Avoid being "confidently wrong" — the cost of a wrong confident answer is higher than admitting uncertainty.
