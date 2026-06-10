---
agent_doc_session: tsift-v0.1
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260610-185850.md`*

Compacted content:
- Archived 1 response topic(s): next-steps closeout repair
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: queue duplicate cleanup — gpt-5

> **Queue prompt:**
>
> [#hccr2]
>
> [#hsmart2]
>
> [#hcache2]
>
> [#hlearn2]
>
> [#hmcp2]


Preflight found the current queue head `#hccr2` is already satisfied by archived completion `#hdrccr`. I checked the sibling headroom-derived items too and closed the duplicated set: `#hccr2`, `#hsmart2`, `#hcache2`, `#hlearn2`, and `#hmcp2`.

No source files changed in this cycle. Commands/proof: `agent-doc commit /home/brian/work/btakita/agent-loop/tasks/software/tsift.md` repaired prior queue metadata drift; `tsift status` and `tsift index --workspace .` refreshed navigation; `rg` confirmed the archived matches in `tasks/software/tsift.done.md`. No build/test run was needed because this was backlog cleanup only.

Verification confidence: high for duplicate cleanup. Escalation: none. `#cmmcp-eval` remains queued as the next substantive tsift evaluation item.

### Re: codebase-memory-mcp evaluation — gpt-5

> **Queue prompt:**
>
> :pushpin: [#cmmcp-eval]


I evaluated DeusData/codebase-memory-mcp as an idea source, not as a tsift replacement. It is still active and current enough to matter: the GitHub README/repo advertises v0.7.0, 159 tree-sitter languages, 14 MCP tools, static binaries, local SQLite graph storage, bundled semantic/BM25 search, agent install support, and a shareable `.codebase-memory/graph.db.zst` artifact. Release notes also show graph-augmented `search_code` work and the compressed artifact path. Sources checked: https://github.com/DeusData/codebase-memory-mcp, https://github.com/DeusData/codebase-memory-mcp/releases, https://github.com/DeusData/codebase-memory-mcp/issues/277, plus local `src/tsift/README.md`, `src/tsift/specs/graph.md`, and `src/tsift/specs/search-navigation.md`.

Verdict: port ideas selectively. tsift should keep its edge around agent-doc integration, bounded envelopes, digest-runner/session-review/diff-digest, scale guards, source handles, conflict-matrix/dependency-dag, provider-neutral GraphStore, and explicit freshness/doctor contracts. codebase-memory-mcp is strongest as evidence that agents benefit from a direct graph query surface, a committed compressed graph bootstrap, graph-augmented grep/search, and polished single-command MCP/install UX.

Port-worthy follow-ups added to backlog in priority order: `#cbmsnap`, `#cbmquery`, `#cbmsearch`, and `#cbmtrust`. I did not add a new MCP-tools item because the existing archived `#hdrmcp` already covers first-party tsift MCP tools. I also would not port the pure-C architecture itself; for tsift the useful part is install/trust ergonomics, not rewriting the Rust crate stack.

Important caution: codebase-memory-mcp's own issue history includes WAL/orphan-process stale-index failure modes on Windows. tsift already has stronger WAL-aware snapshot fallback, `graph-db doctor`, freshness, schema-version, compaction, and semantic-readiness checks, and my local `tsift graph-db --path . --json status` currently reports a stale schema plus summary-cache readiness block. Any shareable snapshot work should import through those checks and fail closed when projection metadata or semantic readiness is stale.

Changed paths: session document/backlog only. Commands/proof: `agent_doc_preflight`, `agent_doc_plan`, web/source review of codebase-memory-mcp, `tsift --envelope search`, local spec reads, `git status`, and `tsift graph-db --path . --json status`. Verification: no build/test run was needed because this cycle did not change source, tests, build logic, or instruction surfaces. Confidence: high for the evaluation and backlog triage; medium on upstream performance/security claims because I verified project documentation and issue history, not an independent benchmark run. Escalation: none.
<!-- agent:boundary:847aeda7 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- [#cbmsnap]
- [#cbmquery]
- [#cbmsearch]
- [#cbmtrust]
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#cbmsnap] [recommended] Prototype a shareable compressed tsift graph snapshot artifact that imports through graph-db doctor/freshness checks, preserves projection_version/content_hash/source_watermark, and never bypasses WAL/lock/semantic-readiness diagnostics.
- [ ] [#cbmquery] [recommended] Spike a constrained read-only graph query DSL for tsift graph-db over the stable node/edge/property schema; compare Cypher-like expressiveness against existing node/kind/neighborhood/path/map commands before committing syntax.
- [ ] [#cbmsearch] [recommended] Add a graph-augmented exact/search preview mode that expands grep hits to containing symbol/source_handle windows, ranks definitions and high-signal symbols before tests, and keeps scale_guard/retrieval handles intact.
- [ ] [#cbmtrust] [recommended] Compare codebase-memory-mcp's single-binary install/trust UX against tsift release/install flow, including checksums, attestations/scans, and idempotent agent config upserts, without changing tsift's Rust architecture.
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
