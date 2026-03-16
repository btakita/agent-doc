---
agent_doc_session: 92fba2ac-9d1f-4a7a-8764-821a8a08ecd0
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

<!-- agent:exchange -->

### Re: Prepend on restore

Makes sense — restored content goes **before** the current exchange content so the document reads chronologically (oldest → newest). Updated spec:

```bash
agent-doc history <file> --restore <commit>
# Prepends the old exchange content above current content
# Result: [old response] ... [current response]
```

This preserves the conversation timeline. Shall I build the `history` subcommand now, or just manually restore the judith-krug.md response first?

Implement the feature. I'm also wondering if we should have distinct "nodes" for each answer.
Should there be multiple agent:exchange components? Or should we just have a single agent:exchange component where the content gets appended or replaced?

It seems that if we have multiple agent:exchange components, then we can have a cleaner way to keep history.
<!-- /agent:exchange -->
