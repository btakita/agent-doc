---
agent_doc_session: 92fba2ac-9d1f-4a7a-8764-821a8a08ecd0
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

<!-- agent:exchange -->

### Flash notification added

Added `tmux display-message` to `route.rs::send_command()` — shows a brief flash on the target pane immediately when the route dispatches the command:

```
⏳ /agent-doc youtube.md
```

This appears for 2 seconds, giving instant visual feedback that the command was sent, even before Claude Code picks up the input. Uses just the filename (not full path) to keep it short.

All 352 tests pass, clippy clean. Want me to include this in the next release?
<!-- /agent:exchange -->
