---
agent_doc_session: 92fba2ac-9d1f-4a7a-8764-821a8a08ecd0
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

<!-- agent:exchange -->

### `agent-doc history` implemented

9 new tests, all 361 pass, clippy clean. Live test on this document:

```
COMMIT       DATE                       EXCHANGE
--------------------------------------------------------------------------------
0c57bcf79bce 2026-03-16 00:08:50 -0400  ### Re: Exchange nodes + history design
c137f9a805e9 2026-03-16 00:06:53 -0400  ### Re: Prepend on restore
2df30fe12295 2026-03-16 00:00:49 -0400  ### Re: agent-doc command for restoring responses
073c009d4934 2026-03-15 23:58:14 -0400  ### Re: Keeping previous responses + restoring
17182cadce98 2026-03-15 23:56:01 -0400  ### v0.17.10 Released
b91b6dcbcc61 2026-03-15 23:35:14 -0400  ### Implemented: Append-mode exchange + findings pipeline
...
```

**Usage:**
- `agent-doc history <file>` — list exchange versions
- `agent-doc history <file> --restore <commit>` — prepend old exchange content with `---` separator

Ready to release v0.17.11?

Can we make it easy to keep what history is currently in the document?
Can we make it easy to replace what history is currently in the document? (current behavior)
We also just had a case where the document was externally edited. So I lost what I was typing. We should **always** use CRDT edits in process via the plugin when available.
<!-- /agent:exchange -->
