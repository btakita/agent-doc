---
agent_doc_session: 92fba2ac-9d1f-4a7a-8764-821a8a08ecd0
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

In tasks/briantakita.me.md, the CRDT created duplicate data. Please add tests to replicate. Fix bug.
```
Why were the videos not public to begin with?
Cork

Why were the videos not public to begin with?
Corky should always publish public videos...unless explicitly to private.
```

<!-- agent:exchange -->

### Published v0.17.3

- **crates.io**: `agent-doc v0.17.3` ✓
- **PyPI**: `agent-doc 0.17.3` ✓
- **GitHub Release**: [v0.17.3](https://github.com/btakita/agent-doc/releases/tag/v0.17.3) with prebuilt binary ✓
- **Release binary built**: `cargo build --release` ✓

Fix: CRDT base advancement no longer splits formatting sequences mid-line — prevents character-level interleaving during concurrent edits.

When I did `agent-doc run tasks/rappstack.md`, the orientation of the tmux panes switched from horizontal (left | right) to vertical (top/bottom).
Please diagnose from logs.
Please add test(s) replicating the issue.
Please fix.

And it just happened.
<!-- /agent:exchange -->
