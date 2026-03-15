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

### Re: External modification fixed

**Root cause:** Only `agent-doc write --ipc` used the IPC path. `agent-doc run` and `agent-doc stream` always wrote directly to disk, bypassing the JetBrains plugin.

**Fix:**
- `submit.rs` + `stream.rs` now try IPC first, fall back to disk write if no plugin
- `write.rs` added `try_ipc()` / `try_ipc_full_content()` helpers
- `PatchWatcher.kt` added `fullContent` support for append-mode docs
- 4 new tests, all 344 pass

Ready to publish.

release
<!-- /agent:exchange -->
