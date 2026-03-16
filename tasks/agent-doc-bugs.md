---
agent_doc_session: 92fba2ac-9d1f-4a7a-8764-821a8a08ecd0
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

<!-- agent:exchange -->

### Re: Append mode + findings pipeline

Good direction. Here's the proposed architecture:

**Two-component pipeline:**

| Component | Mode | Purpose |
|-----------|------|---------|
| `exchange` | **append** | Conversation — each cycle appends, nothing lost |
| `findings` | **append** | Persistent data store — key discoveries, research results |

**How it works:**
1. User writes in `exchange`, agent appends response below
2. When the agent discovers important data (search results, contract terms, etc.), it also patches `findings` to accumulate the data
3. `exchange` can be cleared periodically (by user or agent) when conversation gets long — `findings` persists independently
4. On "inquiry complete," the agent can summarize `findings` into a final deliverable

**Implementation:**
- **Skill change:** Format responses with two patch blocks — `exchange` for conversation, `findings` for data
- **Binary change:** Default `components.toml` sets `exchange` and `findings` to `append` mode
- **Claim change:** `agent-doc claim` scaffolds both components when creating new documents

This is a skill + convention change — no binary code changes needed for the basic version (the skill just uses append-mode patch blocks). The binary change would make it the default for all new documents.

Want me to implement this? Start with the skill (SKILL.md update) or the binary (components.toml defaults)?

Implements
<!-- /agent:exchange -->
