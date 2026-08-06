---
name: feedback_conformance_tests_drive_real_behavior_not_runner_bookkeeping
description: "Conformance runners must assert library behavior, not the runner's own bookkeeping — counters go inside the library's call path, and mutation-check the runner"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 24f78fda-2db5-41b6-8eb1-52772d913df8
  modified: 2026-07-25T22:10:52.056Z
---

When writing conformance/fixture runners, drive real library behavior as much as possible. Brian
asked "Are we over-mocking the conformance tests?" while lazily-cs's reactive-graph runner counted
`merges_of` by incrementing in the *runner* before each `Merge()` call — which is what the
lazily-rs reference runner does, and is weaker than it looks.

**Why:** a counter that increments on the runner's *intention* cannot detect a binding that
swallowed the work. The merge fixtures exist specifically to catch "a binding deferring the fold
itself shows merges_of 1" — the runner-side counter would report 3 either way.

**How to apply:**
- Install behaviour counters *inside the library's own call path* (there: wrap the `MergePolicy`'s
  fold with the counter via `policy with { Merge = ... }`, so the count is folds the library
  performed).
- Mutation-check every new runner before trusting it: break exactly the thing the fixture exists
  to catch, confirm the suite goes red, restore. In lazily-cs, breaking `Merge` to keep-latest
  failed 1 test; cutting the invalidation cone walk to one level failed 6.
- Absence of the corpus is a hard failure, not a skip; assert a positive fixture count and an
  assertion floor. "Green" and "ran nothing" must not be indistinguishable.

See [[reference_lazily_conformance_delivery_three_failure_modes]] for the three ways a suite
reports green while testing nothing, and [[project_lazily_cs_binding]] for the repo this came up in.
