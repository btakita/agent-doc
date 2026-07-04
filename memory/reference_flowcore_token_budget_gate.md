---
name: reference-flowcore-token-budget-gate
description: agent-doc has a test that fails when guard_/proof= token counts change in hot-path files; bump the budget with an audit comment
metadata: 
  node_type: memory
  type: reference
  originSessionId: e7a9180a-1d53-4dbd-9734-9ecfd6c44ebe
---

agent-doc's `tests/test_cli.rs::flowcore_hot_path_guard_and_proof_tokens_are_budgeted`
counts occurrences of the tokens `guard_`, `proof=`, `proof_scope=`, `reason=`,
`flow_reason=`, `accepted_only` in seven hot-path source files (git.rs,
orchestrate.rs, preflight.rs, repair.rs, route.rs, session_check.rs, write.rs)
and asserts each matches an exact expected count in `flowcore_hot_path_token_budget`.

**How to apply:** Any new ops-log diagnostic (`*_guard_fired`), guard fn, or test
name containing one of those tokens in those files breaks the gate. Fix by editing
`flowcore_hot_path_token_budget` to the new count and adding a `// +N for the
audited #<bug-id> ...` comment above the entry (mirror the existing comment style).
Watch out: calling `resolve_pending_done_guard_mode` adds a `guard_` too, not just
the obvious diagnostic/test names — count every substring. `make check` is the only
thing that catches this; the crate compiles fine. Hit while landing
[[reference-pypi-publish-via-ci]]-style work on #blocked-closeout-followup-capture
(session_check `guard_` 40 → 49).
