---
agent_doc_session: bef800bc-950e-4f9e-b861-92c249853397
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
`related_docs` → `links` rename complete (v0.28.3). URL link support not yet implemented.
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
### Re: related_docs already exists

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
Before we do that, would it be better to rename `related_docs` to `linked`?
`linked` would also imply other sources such as urls, documents with other types of formats, etc.
I think it's actually a good idea to link these other types of sources.
### Re: Rename to `linked` + expand scope

Good thinking. `linked` is more general and future-proof:

We should rename `linked` to `links`.

```yaml
---
linked:
  - ../professional/bob-remeika.md          # relative path
  - tasks/software/agent-doc.md             # project-relative
  - https://docs.example.com/api            # URL
  - mail/contacts/ron-berkes/AGENTS.md      # non-agent-doc file
---
```

**Benefits of `linked` over `related_docs`:**
- Shorter, cleaner
- Implies active linking (not just "related")
- Natural extension to URLs, non-.md files, external resources
- The preflight can handle each type differently: local files → diff, URLs → fetch + cache

**Migration:** Keep `related_docs` as a backward-compatible alias via `#[serde(alias = "related_docs")]`. New docs use `linked`.

**Implementation scope:**
1. Rename field + alias (small change)
2. URL support in preflight — `WebFetch` or `curl` for URLs, cache in `.agent-doc/linked_cache/`
3. Non-.md file support — read content, include in preflight context

This is a feature design that should go into `tasks/software/agent-doc.md` for proper planning. Want me to add it there, or implement the rename here first?
This document will handle the feature.
One of the features of `related_docs` is changes to context would ripple across the related_docs.
How would `links` handle these updates? Would editable files be edited? How would the info in uneditable files, such as urls, be handled?
I suppose that we can see if the agent-doc repo can edit the source of the url...such as briantakita.me. But some of the urls would be uneditable.
### Re: `links` design — editable vs read-only sources

Good name — `links` is cleaner. Here's the design for how different source types are handled:

**Link types by editability:**

| Source | Read | Write | Change detection |
|--------|------|-------|-----------------|
| Local .md (agent-doc) | Preflight diff | `agent-doc write` via patch | Snapshot-based diff |
| Local file (non-.md) | Read content | Direct file edit | mtime + hash |
| URL (editable repo) | Fetch + cache | Git push to source repo | Cache diff vs fresh fetch |
| URL (read-only) | Fetch + cache | Cannot edit — context only | Cache diff vs fresh fetch |

**How changes ripple:**

1. **Local .md links:** Already implemented via `check_related_docs()`. Changes in linked docs appear in the preflight diff. The agent can respond to those changes and update the current doc OR write back to the linked doc.

2. **Local non-.md files:** Read the content during preflight, include as context. If the agent wants to update the linked file, it can use the Edit tool directly (not via agent-doc write).

3. **Editable URLs (e.g., briantakita.me):** If the URL maps to a known git repo (configured in the link metadata), the agent can clone/pull, edit, and push. This is a heavier operation — probably triggered explicitly, not on every preflight.

4. **Read-only URLs:** Fetch content, cache in `.agent-doc/links_cache/`, include as context. The agent can reference this content but can't edit the source. Changes detected by comparing cached vs fresh fetch.

**Frontmatter syntax:**
```yaml
---
links:
  - ../professional/bob-remeika.md           # local file (auto-detected)
  - https://briantakita.me/about             # URL (auto-detected)
  - url: https://example.com/api
    editable: false                           # explicit read-only
  - path: ../some-repo/README.md
    repo: git@github.com:user/repo.git       # editable via git
---
```

Simple entries (strings) are auto-detected: paths → local file, URLs → fetch+cache. Object entries allow metadata (editable flag, repo URL, refresh interval).

Want me to implement the rename (`related_docs` → `links` with alias) first as a foundation, then build out the URL support?
Yes

We lost the active claude session running the task `Yes` on the document.
Why?
Is it currently running in the background?
I did notice that the visible tmux window a Claude did
### Re: Lost session + rename status

**The lost session:** The previous Claude session that received your "Yes" was running in a tmux pane managed by `agent-doc start`. When a session dies (timeout, crash, network issue), the pane goes idle and `agent-doc` doesn't auto-restart — you have to re-invoke `/agent-doc` to pick up where it left off. That's what just happened.

**Is it running in the background?** No. The stash pane %275 is running `tasks/software/corky.md`, not this document. The previous session for this document is gone.

**Rename status:** The `related_docs` → `links` rename is **already done** (committed as `9b87df1` in agent-doc). The `Frontmatter` struct now has:
```rust
#[serde(default, skip_serializing_if = "Vec::is_empty", alias = "related_docs")]
pub links: Vec<String>,
```

Existing documents using `related_docs:` in frontmatter will continue to work via the alias. New documents should use `links:`.

**Remaining items:**
- URL link support (fetch + cache + include as context in preflight)
- Editable repo links (git push back to source)
- Object entry syntax (metadata like `editable`, `repo`, `refresh`)

Want me to start on URL link support next?
<!-- agent:boundary:54fced25 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] v0.28.0-v0.28.3: All shipped
- [x] **Rename `related_docs` → `links` (with backward-compat alias)** — done in v0.28.3
- [ ] **URL link support (fetch + cache + context)**
- [ ] **Object entry syntax** (editable flag, repo URL, refresh interval)
- [ ] **Editable repo links (git push back)**
<!-- /agent:pending -->
