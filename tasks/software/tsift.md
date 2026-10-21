---
agent_doc_session: tsift-v0.1
resume:
  claude: 2f70e722-90c5-4918-b28b-0cf7bc1660eb
  codex: 01a15c95-31b2-79b0-a7cc-b2aa26f82720
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#bugs-observed': Please add any bugs from observed behavior to the backlog.,
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#gh-fix': fix then close
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#subagent': 'run in a subagent'
  '#subagents': 'run the remaining items in subagents'
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: go
---

## Status

<!-- agent:status patch=replace -->

complete
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20261021-035050.md`*

Compacted content:
- Archived 6 response topic(s): GitHub issues 26–28; v0.1.104 release; upgrade-friendly tsift skill; 3 more
- Prior summary/context: prior compacted content: Archived 10 response topic(s): [#haivenrootsync] dispatched to a subagent; [#haivenrootsync]...

### Re: recovered response

Session is now assigned to Codex. `#gh-fix` for [tsift issue #29](https://github.com/btakita/tsift/issues/29) is queued for the next agent-doc cycle.
> **Queue prompt:** release + publish: include Issue #29

### Re: v0.1.106 release and Issue #29 — gpt-5 · 2026-10-21T01:06:46-04:00

Released `v0.1.106` from `8dfa9c5`; [Issue #29](https://github.com/btakita/tsift/issues/29) is fixed by `f9c92e7` and closed.

- Replaced mtime-based Markdown freshness with content-hashed watermarks, captured the source snapshot before traversal reads, and made concurrent source drift nonfatal for refresh while preserving fail-closed reads.
- Published the [GitHub release](https://github.com/btakita/tsift/releases/tag/v0.1.106) with six binary/checksum assets, all workspace crates to crates.io, and `opencode-tsift@0.1.106` to npm.
- Verified the full `make check` suite, a local `tsift 0.1.106` install, main CI, the successful [release workflow](https://github.com/btakita/tsift/actions/runs/39544595646), registry visibility, release assets, and issue closure.

### Re: do [#databasefiledecisions] — gpt-5 · 2026-10-21T02:35-04:00

Yes. Decision knowledge should be a first-class **tsift feature**, not a separate user-facing tool. The clean design is a dual representation:

- **Version-controlled Markdown/ADR files are authoritative.** They are reviewable, mergeable, portable, and readable without tsift.
- **`.tsift/memory.db` and `.tsift/graph.db` are derived retrieval indexes.** They provide lexical/semantic lookup, code-symbol anchoring, relationships, recency, and bounded context injection, but should not be the only copy of a durable project decision.

This is already the direction of the codebase. `tsift-memory` has a SQLite memory-event store; the shared graph supports authored `finding`, `decision`, and `note` nodes anchored to stable symbol handles; and the CLI already exposes `tsift memory finding-add --kind decision`, `tsift memory findings --kind decision`, and `tsift graph-db ... related`. A separate product would duplicate storage, provenance, graph linkage, and retrieval machinery that tsift already owns.

The missing layer is a durable decision contract and file importer. A useful first version would:

1. Index a conventional repository path such as `decisions/**/*.md` (and optionally existing ADR directories).
2. Give every decision a stable ID, status (`proposed`, `accepted`, `superseded`, `rejected`), statement, rationale, consequences, code/source anchors, provenance, and optional `supersedes` links.
3. Project those records into the shared graph with content hashes, making the projection rebuildable from files.
4. Let `context-pack`, search, and `graph-db related` retrieve only the relevant accepted decisions within a token budget.
5. Keep transient session observations in `memory.db`; promote them to an authoritative decision file only when they become durable project policy.

A reusable internal crate such as `tsift-decisions` could be worthwhile once parsing and lifecycle rules grow, but it should remain behind the tsift CLI/API. A genuinely separate service is justified only for organization-wide approval workflows, permissions, or cross-repository governance; tsift can ingest or federate those records rather than owning that workflow.

So the recommendation is: **file as source of truth, tsift database as projection, tsift as the product boundary**.

Evidence: no repository paths changed because this cycle was classified as a design response. I verified the current boundary with `tsift status`, bounded tsift searches/source reads, `tsift memory findings --help`, `tsift memory finding-add --help`, and `tsift memory status . --json`. Confidence is high; no escalation is needed.

### Re: do [#needfeaturesdecisions] — gpt-5 · 2026-10-21T02:50-04:00

Yes, but the Markdown should stay deliberately small. Its job is to be the reviewable, source-controlled record; tsift should derive the richer retrieval graph from it.

A good v1 needs:

1. **Stable identity and lifecycle** — `id`, `status` (`proposed`, `accepted`, `superseded`, `rejected`), and `date`. Decision lifecycle should remain distinct from tsift's ingestion trust (`draft`/`trusted`); accepted files can project as trusted, while proposed files remain draft-only.
2. **Consistent semantic sections** — `Decision`, `Context`, `Rationale`, and `Consequences`; `Alternatives` can be optional. This gives agents predictable, bounded fields instead of forcing them to summarize arbitrary prose.
3. **Scope and anchors** — affected repositories, paths, and stable tsift symbol handles. Never use line numbers as durable anchors.
4. **Relationships** — `supersedes`, `superseded_by`, and `related` decision IDs. These should become graph edges, with validation for missing IDs and cycles.
5. **Provenance** — author/source, originating issue or session, and optional commit/reference links. Generated database fields and embeddings should not be written back into Markdown.
6. **Validation and synchronization** — a command such as `tsift decision lint|sync` should validate schema and links, project changed files idempotently into `.tsift/findings.db` / the shared graph, detect removed or renamed decisions, and report drift without silently rewriting authored prose.
7. **Retrieval policy** — search and `context-pack` should prefer accepted, non-superseded decisions relevant to the current code/query, while allowing explicit inclusion of proposed or historical records. Results should cite the Markdown path and decision ID.

A compact file could look like:

```markdown
---
id: dec-0007
status: accepted
date: 2026-10-21
anchors:
  - code_symbol:GraphStore
supersedes: []
related: [dec-0003]
---

# Keep decision records file-backed

## Decision
Use tracked Markdown as authority and tsift databases as projections.

## Context
...

## Rationale
...

## Consequences
...
```

The current `tsift finding` layer already supplies most of the database behavior: decision nodes, draft/trusted promotion, relationships, stable symbol/file anchors, stale-anchor detection, passive harvesting, and a dedicated `.tsift/findings.db`. The missing feature is therefore a Markdown importer/exporter plus schema/link linting—not another database or another standalone tool.

Features I would defer from v1: approval workflows, organization-wide ACLs, elaborate templates, automatic prose rewriting, and mandatory one-file-per-decision layout. Those can be layered later without changing the core record contract.

Evidence: no repository paths changed because the planning contract classified this cycle as a design response. I verified the existing surface with `tsift status`, bounded searches/source reads, and the `FindingCommand` definition. Confidence is high; no escalation is needed.
<!-- agent:boundary:bf6d00d6 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ~~do [#needfeaturesdecisions]~~
- ~~do [#databasefiledecisions]~~
- #gh-fix https://github.com/btakita/tsift/issues/29
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#g5kw] What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
- [ ] [#4z0w] Can we increase max-hop caps?
- [ ] [#cg52] Could FalkorDB help with performance?
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->

<!-- agent:notes -->
• Ran tsift rewrite --run 'git log -5 --oneline --decorate'
└ tsift rewrite: no rewrite: no supported tsift rewrite matched this command; `--run` executes only rewritten commands; run the original command directly if intended

#spec-test-build-install-commit-push
---
route duplicate prompt residue guard failed: failed to parse components: unclosed component: `agent:notes` without matching close
<!-- /agent:notes -->
