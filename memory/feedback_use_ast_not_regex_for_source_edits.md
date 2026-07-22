---
name: feedback-use-ast-not-regex-for-source-edits
description: "Use AST tooling (tsift edit-intents), never regex, for structured source edits — regex silently corrupted Rust in 3 ways in one session"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: cc599a25-2160-4df4-a361-284399027114
---

Never use regex/sed to transform structured source (Rust, markdown, any AST-backed
language). Use the AST: `tsift edit-intents --apply --verify` applies semantic edit
intents and verifies them in a temporary git worktree before mutating the tree.
`tsift edit` applies batched file edits from JSON.

**Why:** 2026-07-19, migrating `std::sync::Mutex` → `parking_lot::Mutex` across
agent-doc, regex failed three distinct ways in minutes — each one silent until the
compiler happened to catch it:
- ate the inner `Err(_)` arms of nested `match` expressions (an inner arm looks
  identical to the outer one being removed)
- matched `.lock().map_err(…poisoned…)?` on *unrelated* std Mutexes in three files
  outside the intended scope (`controller memory lock`, `controller bootstrap lock`,
  `force-disk authority baseline lock`) — required `git checkout` to undo
- truncated a whole function body (`agent_doc_replica_persist`) by mismatching
  delimiters

Two `git checkout` reverts. Brace-counting Python was better than regex but still
not an AST.

**How to apply:** for any multi-site source transform, reach for `tsift edit-intents`
first. If a transform genuinely can't be expressed as an intent, do the sites by hand
with Edit and read the full diff before committing — a clean `cargo build` is NOT
evidence the transform was semantically faithful. Prose (doc comments, messages) is
fine to edit by hand. See [[feedback_check_working_tree]].
