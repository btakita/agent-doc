---
agent_doc_session: 92fba2ac-9d1f-4a7a-8764-821a8a08ecd0
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

<!-- agent:exchange -->

### Released v0.17.7

- crates.io: https://crates.io/crates/agent-doc/0.17.7
- PyPI: https://pypi.org/project/agent-doc/0.17.7/
- GitHub: https://github.com/btakita/agent-doc/releases/tag/v0.17.7 (with prebuilt binary)
- Release binary built and symlink updated

The agent response to agent:exchange should provide more context into what this release was about.
I do like the clean-slate approach. But I also wonder what happens to the response.
Do I completely lose it?
I could always ask you to show the old response. But I wonder if a short reminder or log should be printed.
Perhaps in a log area.

Also there's content above the `agent:exchange` component that has stuck around from the beginning of this document.
I just deleted it.
Perhaps we should have a status component on the document by default.
So each document will have `agent:status` and `agent:exchange` by default.
Should this be in the agent-doc binary or skill?s
<!-- /agent:exchange -->
