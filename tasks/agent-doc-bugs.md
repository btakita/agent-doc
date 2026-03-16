---
agent_doc_session: 92fba2ac-9d1f-4a7a-8764-821a8a08ecd0
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

<!-- agent:exchange -->

### Re: Settings naming + append mode

**Naming consolidation:** You're right — having `append` in both `agent_doc_format` and `agent_doc_response` is confusing. Here's a cleaner model:

| Setting | Values | Meaning |
|---------|--------|---------|
| `agent_doc_format` | `template` \| `append` | Document structure: components vs User/Assistant blocks |
| `agent_doc_response` | `replace` \| `append` | How agent responses update the exchange: overwrite vs accumulate |
| `agent_doc_write` | `crdt` \| `merge` | Write strategy: CRDT conflict-free vs 3-way merge |

`agent_doc_format: template | inline`...so inline instead of append. Migrate existing values of `append` to `inline`.

The overlap is `append` meaning different things:
- `agent_doc_format: append` → document-level structure (alternating blocks)
- `agent_doc_response: append` → component-level behavior (accumulate responses)

**Better name for `agent_doc_response`?** Options:
- `agent_doc_response` (your proposal) — clear but slightly ambiguous
- `agent_doc_accumulate` — `true`/`false` (boolean, simpler)
- `agent_doc_exchange` — `append`/`replace` (scoped to exchange component)

I like `agent_doc_exchange: append | replace`
I'm also considering `agent:exchange: append | replace` because the tag is `agent:exchange`.
Can we explore component level configurations in this way?

Another possibility is `agent:exchange[mode=append|replace]` or `agent:exchange mode="append"|"replace"`
Which is probably better than the front matter.
Let's have the default be `agent:exchange mode=append`

I'd go with `agent_doc_response: append|replace` — it's the most intuitive. The "append" overlap with format is acceptable because they operate at different levels (document vs component).

**For this document:** I'll treat it as append mode from now on. Previous responses will accumulate.

**Restoring history:** I'll pull the previous responses from git and prepend them. And I'll implement the squash command next.
<!-- /agent:exchange -->
