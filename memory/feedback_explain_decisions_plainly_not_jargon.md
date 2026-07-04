---
name: feedback_explain_decisions_plainly_not_jargon
description: "When surfacing open decisions to the user, explain them in plain language with a recommended default — don't bury them in internal jargon"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: e1015ac9-dcc7-4dfa-a368-63235f718e84
---

When asking the user to make a decision, do NOT name it only with internal shorthand ("Stage 2", "review-lint Stage 2", "Phase 5b write-side lifecycle"). The user said verbatim: "I have no idea what Stage 2, review-lint Stage 2, Phase 5b write-site lifecycle mean."

**Why:** internal phase/stage labels are meaningless to the user; they carry the implementer's context, not theirs. Listing them as "open decisions" without explanation creates work and confusion instead of enabling a choice.

**How to apply:** for every decision surfaced to the user, give (1) a one-sentence plain-English description of what's actually being chosen and why it matters, and (2) a recommended default so the user can simply approve. Default to deciding it yourself with a sensible default and letting them override — especially when the user signals they don't want to engage with the details. Reserve [[AskUserQuestion]]-style prompts for choices that genuinely change the outcome and that the user is positioned to judge in their own terms.
