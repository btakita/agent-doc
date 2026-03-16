---
agent_doc_session: 92fba2ac-9d1f-4a7a-8764-821a8a08ecd0
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

<!-- agent:exchange -->

### Re: Exchange nodes + history design

**Single exchange with node boundaries** is cleaner than multiple components. Here's why:

Multiple `agent:exchange-N` components would require dynamic component names, breaking the parser's fixed-name model and complicating `components.toml`. Instead:

**Node model:** A single `exchange` component where each response is a "node" separated by `---` (horizontal rule). Each node maps to a git commit:

```markdown
<!-- agent:exchange -->
### Re: First topic
Response one content.

---

### Re: Second topic
Response two content.

---

### Re: Third topic
Latest response.
<!-- /agent:exchange -->
```

**History command uses git commits as node boundaries:**
- `agent-doc history <file>` — lists commits with first line of each exchange version
- `agent-doc history <file> --restore <commit>` — prepends that commit's exchange content above `---` separator

**Benefits:**
- Single component, no parser changes
- Git history = authoritative node timeline
- `---` separators are visual + parseable
- Append mode naturally accumulates nodes

I'll implement `agent-doc history` with this model. The `---` separator convention is a skill convention (the binary just does git extraction + prepend).

Should the topic name also include the git sha?
<!-- /agent:exchange -->
