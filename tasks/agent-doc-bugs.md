---
agent_doc_session: 92fba2ac-9d1f-4a7a-8764-821a8a08ecd0
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

<!-- agent:exchange -->

### Re: Template + append exchange + squash

**Template + append exchange:** This is already how it works with v0.17.10! Your document has `agent_doc_format: template` in frontmatter, and `.agent-doc/components.toml` sets `exchange` to `mode = "append"`. Each cycle the skill writes a `<!-- patch:exchange -->` block that gets appended (not replaced) to the exchange component.

Check your `components.toml`:
```toml
[exchange]
mode = "append"    # ← this is what makes exchange append

[findings]
mode = "append"

[status]
mode = "replace"
```

I want to discuss the settings:
```
agent_doc_format: template | append
agent_doc_response: replace | append
agent_doc_write: crdt | merge
```
I renamed mode to `agent_doc_response`. Is there a better name?
This is strange because we now have `append` being a valid value for both `agent_doc_format` and `agent_doc_response`.
I wonder if we can consolidate.

If this file doesn't exist for a document, run `agent-doc claim <file>` to create it with defaults.

**Squash command:** Not yet, but it's a natural complement to `history`. Proposed:

```bash
# Squash all exchange content to just the latest response
agent-doc squash <file>

# Squash to a specific commit's content
agent-doc squash <file> --from <commit>
```

This would clear the exchange component and write only the latest (or specified) response. The old content survives in git history and can be restored with `agent-doc history --restore`.

Should I implement `squash` now, or focus on the IPC timeout fix + release first? The IPC agent is still building.

Both

Please add the response that I replaced in the history.
Let's go into `agent_doc_response: append`
<!-- /agent:exchange -->
