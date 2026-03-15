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

### Published v0.17.5

- **crates.io**: `agent-doc v0.17.5` ✓
- **PyPI**: `agent-doc 0.17.5` ✓
- **GitHub Release**: [v0.17.5](https://github.com/btakita/agent-doc/releases/tag/v0.17.5) with prebuilt binary ✓
- **Release binary built**: `cargo build --release` ✓

Fix: IPC-first writes — `agent-doc run` and `agent-doc stream` now use JetBrains plugin IPC before falling back to disk writes.

audit docs
<!-- /agent:exchange -->
