---
name: no-hallucinate-correct-errors
description: Don't call broken behavior "correct" — verify against spec before characterizing errors
type: feedback
---

When diagnosing errors, verify the observed behavior against the SPEC before characterizing it as "correct" or "expected." Don't rationalize broken behavior.

**Why:** During 2026-04-04 session, the claim error ("pane already claimed") was described as "the claim correctly refused to prevent corruption" when it actually violated the Binding invariant (SPEC 8.5: "never commandeer another document's pane"). The correct behavior was to provision a new pane. This was the 3rd time this mistake pattern occurred.

**How to apply:**
1. When an error occurs, read the SPEC section that governs the behavior
2. Compare observed behavior against spec — is the error itself spec-compliant?
3. If uncertain, say "I don't know if this is correct behavior" (per no-hallucinate-uncertainty memory)
4. Never say "correctly refused" or "expected error" without verifying against the spec
5. The Binding invariant (8.5) is the foundational rule — always check it for pane/claim issues
