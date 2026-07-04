---
name: feedback_module_split_functionality_with_tests
description: "When splitting large modules, co-locate each functional unit's tests with it — don't extract tests into a separate tests.rs"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 5f221ae6-1975-4b37-b8be-22a4b6f30d63
---

When asked to split large modules into smaller coherent modules, the operator wants **coherent units of functionality + their tests together**, NOT a bare "move `#[cfg(test)] mod tests` into `<mod>/tests.rs`" test-extraction.

**Why:** A `<mod>/tests.rs` holding all of a module's tests (or a giant shared `tests.rs`) is not a coherent unit — the functionality and the tests that prove it should live in the same focused submodule so each split is self-contained.

**How to apply:** Decompose by functionality (e.g. `write/queue_consume.rs`, `write/ipc.rs`, `write/normalize.rs`), and move the test fns that exercise each functional cluster INTO that submodule's own `#[cfg(test)] mod tests`. Pure test-only extraction (logic file shrinks, a separate big `tests.rs` appears) is explicitly NOT what's wanted — only acceptable as a transient step before redistributing the tests to their functional homes.

Stated 2026-06-12 during the agent-doc `write.rs` (28k-line) decomposition. The functional submodule extractions were the right direction; leaving their tests behind in `write/tests.rs` was the part to fix. Related: [[feedback_session_document_scope_discipline]].
