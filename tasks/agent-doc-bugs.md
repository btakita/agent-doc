---
agent_doc_session: 92fba2ac-9d1f-4a7a-8764-821a8a08ecd0
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

<!-- agent:exchange -->

### v0.17.9 Released

CRDT character-interleaving fix shipped:
- crates.io: https://crates.io/crates/agent-doc/0.17.9
- PyPI: https://pypi.org/project/agent-doc/0.17.9/
- GitHub: https://github.com/btakita/agent-doc/releases/tag/v0.17.9

Key change: `compute_edit_ops()` now uses `TextDiff::from_lines()` instead of `from_chars()`. Line-level diffs produce clean merge operations that don't interleave at character boundaries during concurrent edits. All 352 tests pass.

In tasks/youtube/youtube.md, it took a while for the claude session to pick up the agent-doc run command.
Is there a way to get quicker feedback in the claude session terminal?
<!-- /agent:exchange -->
