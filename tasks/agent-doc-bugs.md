---
agent_doc_session: 92fba2ac-9d1f-4a7a-8764-821a8a08ecd0
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

<!-- agent:exchange -->

### CRDT corruption detected (again)

The previous write cycle produced garbled text (lines 23-28) — same character-level interleaving bug. The CRDT merge log showed `File was modified during response generation. CRDT merging...` which indicates concurrent user edits triggered the merge path, and the merge produced corrupted output.

This is another instance of the baseline/CRDT state mismatch issue. The fix from v0.17.6 (using fresh CRDT state from baseline) didn't fully resolve it — the character-level diff still produces overlapping ops in some cases.

**Clean document restored.** The template patch parser code-block fix is still in the codebase (5 new tests, all 351 pass).

Shall I:
1. Release v0.17.8 with the template parser fix (code-block awareness)
2. Deep-dive the remaining CRDT merge corruption

Or both?

Both
<!-- /agent:exchange -->
,.
