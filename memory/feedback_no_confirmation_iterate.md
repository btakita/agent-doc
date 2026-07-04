---
name: No confirmation for obvious next steps
description: Don't ask permission for cargo install or other obvious iteration steps — just do them
type: feedback
originSessionId: 039d83b0-499e-45cc-8d10-b01849fc6662
---
When a fix requires `cargo install --path .` or similar deployment steps to test, just do it. Don't ask "Want me to do that?" — it wastes a turn.

**Why:** User wants fast iteration. Asking permission for obvious next steps breaks flow.

**How to apply:** After building/testing a code fix, immediately run `cargo install --path .` (or equivalent) without asking. Only pause for genuinely ambiguous or destructive actions.
