---
agent_doc_session: 92fba2ac-9d1f-4a7a-8764-821a8a08ecd0
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

<!-- agent:exchange -->

### Re: Slow pickup of agent-doc commands

The delay is between when `agent-doc route` sends keys to the tmux pane and when Claude Code processes the input. This is mostly Claude Code's internal latency — we can't control how fast it picks up typed input.

**Options to reduce perceived latency:**

1. **Watch daemon (already exists):** `agent-doc watch` auto-submits on file save with zero debounce for CRDT docs. Bypasses `/agent-doc` typing entirely — just save the file and the daemon routes the command. Is this what you're already using, or are you triggering manually from the IDE?

2. **Immediate feedback in tmux:** Could add a `tmux display-message` from `agent-doc route` so you see "Sent /agent-doc <file> to pane %X" immediately, even before Claude picks it up. At least you'd know the command was dispatched.

3. **Pre-warm:** If the Claude session is idle for a while, the first input after idle can be slower. No fix for this — it's Claude Code behavior.

Which path are you hitting — IDE plugin → route, or manual `/agent-doc` in the terminal?

IDE plugin → route...
Let's try a flash notification in the tmux pane that receives the run.
<!-- /agent:exchange -->
