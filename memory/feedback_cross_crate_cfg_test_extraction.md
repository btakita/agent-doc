---
name: feedback-cross-crate-cfg-test-extraction
description: "When extracting agent-doc modules into a sibling crate,"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: e4d923e1-5899-4a78-8522-695c3f4fa93e
---

When moving a module from `agent-doc` (bin/lib) into `agent-doc-orchestration` (or any sibling crate), `#[cfg(test)]` items in the moved module become invisible to the *original* crate's tests. Cargo compiles each crate's tests into a separate test binary, and a dependency crate is built in **non-test** mode, so its `#[cfg(test)]` code (statics, test fixtures, in-process runtime short-circuits) is absent from the rlib the downstream crate links.

Three concrete forms hit during the #bz6s big-bang (Direction A increment 6):
1. **Test fixtures** (`TEST_ENV_LOCK`, `IsolatedTmux`): give the downstream crate its own crate-local copy (each test binary is a separate process, so a per-crate process-global lock is correct), or re-import from the originating external crate (`IsolatedTmux` is really `tmux_router::IsolatedTmux`).
2. **In-process runtime short-circuits** (e.g. `project_controller::refresh_supervisor_lease`'s `#[cfg(test)]` block that avoids spawning a real controller): expose them via a `test-support` cargo feature — `#[cfg(any(test, feature = "test-support"))]` — and have the downstream crate enable `features = ["test-support"]` in its `[dev-dependencies]` entry for the sibling crate. Feature unification keeps it off for release builds.
3. **Source-inspection guard tests** that `read_to_string("src/X.rs")`: repoint to the new path (`agent-doc-orchestration/src/X.rs`).

**Why:** the cluster build can pass green while a downstream test silently regresses — confirm by checking whether the test passed at the last committed increment (it failed only after the move). **How to apply:** during any further extraction (Wave 5 dep-prune, future crate splits), grep moved modules for `#[cfg(test)]` runtime blocks and downstream `tests/` for the moved file's symbols/paths before declaring green. Related: [[feedback-precommit-testing]], [[feedback-verify-live-system]].
