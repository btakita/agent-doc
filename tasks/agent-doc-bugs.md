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

### Published v0.17.2

- **crates.io**: `agent-doc v0.17.2` ✓
- **PyPI**: `agent-doc 0.17.2` ✓
- **GitHub Release**: [v0.17.2](https://github.com/btakita/agent-doc/releases/tag/v0.17.2) with prebuilt binary ✓
- **Release binary built**: `cargo build --release` ✓

Fix: CRDT shared prefix advancement — prevents duplication when both merge sides share content added beyond a stale base.

Install: `cargo install agent-doc` or `pip install agent-doc`

Look at tasks/robert-ward.md.
The CRDT had numerous issues while I was typing and the agent-doc overwrote the file.
Note that the file was externally modified. It should have been done entirely with the JetBrains plugin...without being externally modified.
<!-- /agent:exchange -->
