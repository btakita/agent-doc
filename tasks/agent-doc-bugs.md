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

### Re: tmux pane orientation bug

Investigating the `agent-doc run` command's tmux split logic. The issue is likely using `-v` (top/bottom) instead of `-h` (left/right) in the `split-window` command. Fix agent running.

When I did `agent-doc run tasks/rappstack.md`, the orientation of the tmux panes switched from horizontal (left | right) to vertical (top/bottom).
Please diagnose from logs.
Please add test(s) replicating the issue.
Please fix.

And it just happened. Still in the top/bottom orientation after running this file.
If I switch to the editor focus to the right side, it will revert back to horizontal left|right orientation.
<!-- /agent:exchange -->
