---
agent_doc_session: 92fba2ac-9d1f-4a7a-8764-821a8a08ecd0
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

<!-- agent:exchange -->

### v0.17.8 Released + CRDT Fix

**v0.17.8 released** (template parser code-block awareness):
- crates.io, PyPI, GitHub Release with binary
- https://github.com/btakita/agent-doc/releases/tag/v0.17.8

**CRDT corruption root cause found and fixed:**

`compute_edit_ops()` in `crdt.rs` used `TextDiff::from_chars()` — producing ~20 scattered character-level ops when the agent replaces a multi-line block. When merged with user's concurrent edit, these scattered ops interleave at character boundaries, splitting words mid-way.

**One-line fix:** `TextDiff::from_chars()` → `TextDiff::from_lines()`. Line-level ops produce 2 clean operations (delete old lines, insert new lines) instead of 20 scattered character ops. CRDT concurrent inserts now interleave at line boundaries (acceptable) instead of character boundaries (corruption).

New test `merge_replace_vs_insert_no_interleaving` verifies: agent replacement + user insertion both preserved contiguously with no character interleaving. All 352 tests pass.

Ready to release v0.17.9 with the CRDT fix?

go
<!-- /agent:exchange -->
,.
