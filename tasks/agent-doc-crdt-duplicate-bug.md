---
agent_doc_session: e71ca19b-6167-4a51-a5e0-e4a4937b54bd
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

When I call `agent-doc run` with an inline prompt at the bottom of the `<!-- /agent:exchange -->`...the user prompt is duplicated in front of the generated response...and after...at the original position just before the `<!-- /agent:exchange -->`.

This just happened in tasks/rappstack.md. I deleted the duplicate prompt right before `<!-- /agent:exchange -->`:

> R2 custom domain. Switch the environment variable as well.
> Rename the R2 buckets to match the custom domain.

The frontmatter has:

```
agent_doc_format: template
agent_doc_write: crdt
```
