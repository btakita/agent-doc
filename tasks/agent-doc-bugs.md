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

### Published v0.17.4

- **crates.io**: `agent-doc v0.17.4` ✓
- **PyPI**: `agent-doc 0.17.4` ✓
- **GitHub Release**: [v0.17.4](https://github.com/btakita/agent-doc/releases/tag/v0.17.4) with prebuilt binary ✓
- **Release binary built**: `cargo build --release` ✓

Fix: tmux pane orientation — claim/autoclaim sync arranges files side-by-side instead of stacking vertically.

agent-doc run tasks/rebuild-refactor.md caused an external modification to the file.
The CRDT should've been applied in process with the editor via the plugin.
Please diagnose what went wrong in the logs...write tests...and fix.
<!-- /agent:exchange -->
