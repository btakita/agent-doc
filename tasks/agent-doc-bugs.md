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
**CRDT corruption in lazily-rs.md:** This is a concurrent editing race. You were adding a prompt while the agent was writing a response to the same `agent:exchange` section. The CRDT merge produced a fragment `"> cisions, commands, related projects"` — likely a truncated word from "decisions" or similar.

**Analysis:** The `--stream` CRDT merge uses a 3-way diff (baseline vs yours vs agent's). When both sides edit the same region (inside `agent:exchange`), the character-level diff can produce interleaved fragments. This was the same class of bug fixed in v0.17.3 (character interleaving in concurrent edits).

**Questions to investigate:**
1. Was this using agent-doc v0.17.3+ (which has the line-level diffing fix)?
```
❯ agent-doc --version
agent-doc 0.17.6`
```
2. Was the corruption at a section boundary or mid-paragraph?
Section boundary
3. Can we reproduce by having both sides edit the same `agent:exchange` block simultaneously?
I don't know. Probably not reliably.

Want me to look at the lazily-rs.md git history to see the exact merge that corrupted?

Yes
<!-- /agent:exchange -->
