---
agent_doc_session: bef800bc-950e-4f9e-b861-92c249853397
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
v0.28.3 released. Dedup strips boundary markers before comparison.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/4674cbdd322fce519fe66718682225f86b088adb9a6e63aa6f5df754c59a4ef1-20260331-140801.md`*
Does agent-doc support linked documents in the frontmatter?
### Re: Linked documents in frontmatter

No — agent-doc doesn't currently support linked documents in frontmatter. The `Frontmatter` struct has: `session`, `agent`, `model`, `branch`, `format`, `write_mode` — but no field for linked/related documents.

**What would this look like?**
```yaml
---
agent_doc_session: abc123
linked:
  - tasks/software/agent-doc.md
  - tasks/agent-doc/queue.md
---
```

**Use cases:**
- Agent sees related context when responding
- Cross-document sync (changes in one reflected in another)
- Task dependency tracking

Is this what you're thinking? Or a different use case? This would be a new feature — happy to design it.
It looks like there is related_docs. tasks/software/ragie-web-doc.md set related_docs in the frontmatter when I prompted it to.
Either way, when I request to link documents, nothing is added to the frontmatter.
Should we use linked instead of related_docs?
### Re: related_docs already exists (HEAD)

`related_docs` is already implemented in agent-doc. It's in the `Frontmatter` struct (line 252 of frontmatter.rs) and actively used by preflight (preflight.rs:378-408).

**How it works:**
```yaml
---
related_docs:
  - ../professional/bob-remeika.md
---
```

During `agent-doc preflight`, `check_related_docs()` reads the related docs, computes their diffs against their snapshots, and includes any changes in the preflight JSON output. The agent then sees changes from linked documents alongside the main document's diff.

**Naming:** `related_docs` is the canonical field name — it's already in the codebase and used by ragie-web-doc.md. I'd recommend keeping `related_docs` over `linked` since it's already deployed.

**The issue:** When you "requested to link documents" and nothing was added to frontmatter — that was a skill/agent issue (the Claude session didn't know to add the `related_docs` field). The binary supports it; the skill just needs to know the field name.

Want me to add `related_docs` guidance to the SKILL.md so future sessions know to use it?
<!-- agent:boundary:28e4d3ee -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] v0.28.0-v0.28.3: All shipped
- [ ] Add `related_docs` guidance to SKILL.md
<!-- /agent:pending -->
