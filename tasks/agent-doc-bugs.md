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

### Published v0.17.6

- **crates.io**: `agent-doc v0.17.6` ✓
- **PyPI**: `agent-doc 0.17.6` ✓
- **GitHub Release**: [v0.17.6](https://github.com/btakita/agent-doc/releases/tag/v0.17.6) with prebuilt binary ✓
- **Release binary built**: `cargo build --release` ✓
- **Skill installed**: SKILL.md v0.17.6 ✓

Includes: status tracking instructions for `<!-- agent:status -->` components + IPC-first writes docs.

Release agent-doc SKILL.md update

When you created lazily-rs.md, the default values were incorrect. It should've been:
```
agent_doc_format: template
agent_doc_write: crdt
```
<!-- /agent:exchange -->
