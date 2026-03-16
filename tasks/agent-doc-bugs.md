---
agent_doc_session: 92fba2ac-9d1f-4a7a-8764-821a8a08ecd0
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

<!-- agent:exchange -->

### Re: agent-doc command for restoring responses

Yes — an `agent-doc history` command would be natural:

```bash
# List exchange versions from git history
agent-doc history <file>

# Restore a specific version's exchange content (append to current)
agent-doc history <file> --restore <commit>

# Show diff between two exchange versions
agent-doc history <file> --diff <commit1> <commit2>
```

**How it would work:**
1. Walk git log for the file
2. For each commit, extract the exchange component content
3. Show a summary table (commit, date, first line of exchange)
4. `--restore` appends the old content back into the exchange component

For now, the skill can do this manually — I can pull the judith-krug.md exchange from any commit and write it back. Want me to:

1. **Build the `agent-doc history` command** (proper binary subcommand)
2. **Just restore the judith-krug.md response now** via git show + agent-doc write

I would want history to prepend the content before the latest response.
<!-- /agent:exchange -->
