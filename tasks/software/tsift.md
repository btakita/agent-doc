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

### Re: graph-db snapshot artifact prototype — gpt-5

> **Queue prompt:**
>
> [#cbmsnap]


Completed `#cbmsnap`.

I implemented a shareable compressed graph snapshot prototype in `src/tsift`: `tsift graph-db snapshot-export <output.db.gz> [--force]` and `tsift graph-db snapshot-import <artifact.db.gz> [--replace]`. Export validates the live SQLite graph through existing doctor/freshness checks, refuses a recovered WAL/rollback snapshot-read path, materializes a clean SQLite copy with `VACUUM INTO`, gzip-compresses that copy, and reports the preserved `projection_version`, `content_hash`, `source_watermark`, counts, recovery, warnings, and semantic readiness. Import decompresses to a staged DB, validates through doctor/freshness before install, requires `--replace` before overwriting an existing `.tsift/graph.db`, validates again after install, and rolls back on post-install failure.

Changed paths: `src/tsift/packages/tsift-cli/src/cli.rs`, `src/tsift/packages/tsift-cli/src/commands/infra.rs`, `src/tsift/packages/tsift-cli/src/lib.rs`, `src/tsift/specs/graph.md`, plus the parent `src/tsift` gitlink. Added CLI parser coverage, export/import metadata round-trip coverage, and WAL-lock fail-closed coverage. Spec/schema docs now describe the snapshot artifact contract.

Commits: `src/tsift` commit `342baff` (`feat: add graph-db snapshot artifacts`) and parent gitlink commit `634509a79` (`tsift: add graph-db snapshot artifacts`). Existing unrelated dirty state remains in both the submodule and parent repo; I staged and committed only the snapshot-related files and gitlink.

Verification: `cargo test -p tsift-cli graph_db_snapshot -- --nocapture` passed; `make check` passed; latest GitHub Actions CI check was already green (`CI`, run `27287546999`, completed success at `2026-06-10T15:37:26Z`). Confidence: high. Escalation: none.

### Re: graph-db query DSL spike — gpt-5

> **Queue prompt:**
>
> [#cbmquery]


Completed `#cbmquery` as a design spike. Recommendation: do not commit a Cypher-like surface yet. Use a constrained internal query AST first, then add syntax sugar only after real agent workflows prove the shapes.

Comparison against existing graph-db commands: `node`, `edge`, `edges`, `incident`, `kind`, `neighborhood`, `path`, `related`, `evidence`, and `map` already cover the high-signal operations agents use today. A Cypher-like `MATCH (a:kind {k:"v"})-[e:kind]->(b)` form would be more compact for one-hop joins and property predicates, but it adds parser ambiguity, unbounded traversal risk, and expectation baggage from Cypher features tsift should not support (`CREATE`, `SET`, optional matches, arbitrary path expansion, aggregation, and mutation). The value is real, but mostly as a typed query-plan layer, not as immediate public syntax.

Recommended spike boundary: define a read-only `GraphDbQuerySpec`/JSON AST with explicit `from` node selector, optional edge selector, optional `to` node selector, direction, string-equality property filters, required/default `limit`, and bounded path depth. Compile that AST only to existing GraphStore calls and existing CLI semantics: kind/property scans, edge scans, incident scans, bounded neighborhood, and shortest path. Then expose a preview command that returns a query plan plus the equivalent existing `graph-db` commands before any shorthand syntax is accepted.

Syntax guardrails if sugar is later added: one `MATCH` pattern per query, labels map only to stable node `kind`, edge labels map only to stable edge `kind`, properties are string equality only, `RETURN` can only name bound aliases, `LIMIT` is mandatory or defaults low, bounded path syntax must require an upper bound, and the grammar must reject mutation keywords outright. That keeps the DSL provider-neutral and compatible with SQLite/Convex/tokensave instead of becoming a second database language.

Changed paths: none in this cycle. Commands/proof: `agent-doc preflight tasks/software/tsift.md`, `agent_doc_plan`, and `agent_doc_read` for the queued item; graph-db command/schema context came from the immediately preceding `#cbmsnap` source pass. Verification: no build/test rerun was needed because this cycle changed only the session document response. Confidence: high for the recommendation; medium for final syntax details until a JSON AST prototype is exercised against real agent queries. Escalation: none.

### Re: graph-augmented search preview — gpt-5

> **Queue prompt:**
>
> [#cbmsearch]


Completed `#cbmsearch` with a committed implementation. Search preview hits now preserve the existing `shit-*` retrieval handles while adding nested bounded source context: line-anchored hits expose a `source_handle` window with a concrete `source-read` expansion, and hits inside indexed spans also expose `containing_symbol` metadata with a `symbol-read` expansion. The ranked preview now carries lexical hit line numbers and adds scoring reasons for `source_handle`, `containing_symbol`, `definition_kind`, `source_path`, and `test_path`, so production definitions and high-signal symbols rank ahead of test-only matches without dropping fallback file hits.

Changed paths in the tsift submodule: `packages/tsift-cli/src/search_budget.rs`, `packages/tsift-cli/src/lib.rs`, and `specs/output-formats.md`. Added focused unit coverage for exact-hit source-handle/containing-symbol enrichment and source-definition ranking before tests. Updated the output-format spec to make the new nested hit fields part of the preview contract. Commits: `src/tsift` `0e71761 feat: augment search preview hits`; parent repo `216d1f1c5 tsift: augment search preview hits`.

Verification: `cargo test -p tsift-cli search_budget_ -- --nocapture` passed; `make check` passed, including clippy, full Rust workspace tests/doc-tests, OpenCode npm tests, and npm pack dry run. Latest GitHub Actions check inspected with `gh run list --workflow CI --limit 1`: run `27287546999` completed `success` on `2026-06-10T15:37:26Z`. Residual note: the tsift submodule still has unrelated pre-existing dirty files, including an import-order-only hunk in `packages/tsift-cli/src/search_budget.rs`; those were intentionally left out of this commit. Escalation: none.
<!-- agent:boundary:a838d06e -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- ~~[#cbmsnap]~~
- ~~[#cbmquery]~~
- ~~[#cbmsearch]~~
- [#cbmtrust]
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
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
