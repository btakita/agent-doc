---
name: precommit-testing-discipline
description: Always add tests during implementation, not as an afterthought when user asks
type: feedback
---

Add tests as part of the implementation, not reactively when asked. Follow the precommit runbook step 2 for every code change.

**Why:** During the 2026-04-04 agent-doc-bugs session, P1-P5 features were implemented with basic happy-path tests but edge cases, negative tests, and integration tests (preflight JSON serialization, moved functions) were only added after the user asked "did you add tests?" and "are all edge cases tested?" This created unnecessary back-and-forth cycles.

**How to apply:** Before marking any code task complete:
1. Happy path tests for new functions
2. Negative/edge case tests for classifiers and discriminators
3. Integration tests for JSON serialization (present when set, omitted when None)
4. Direct tests for moved functions in their new module
5. Run `make check` as the final verification
