---
agent_doc_session: tsift-v0.1
resume: 019e85d7-f9ee-79f3-b597-4ed352495bbd
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
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260607-035928.md`*

Compacted content:
- Archived 5 response topic(s): do #trt1 + #memgraphrag-ont; #rankdefault; Can tree-sitter-md be a tsift dependency?; 2 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
Done. I implemented the Phase 2 tsift-side incremental markdown reparse helper.

> **Queue prompt:**
>
> do "*Phase 2c (tsift, optional refinement):* an explicit incremental-reparse helper on top of `parse()` for CRDT keystroke edits (tree-sitter-md is already incremental; today consumers re-`parse()`). Say the word if you want it; I didn't queue a churny item."


What changed:

- Added `MdTextEdit` to `tsift-md-ast` as a serializable byte-range replacement shape for CRDT/live-editor edit events.
- Added `reparse_incremental()` for old-source/new-source edits and `reparse_incremental_with_input_edit()` for callers that already have a `tree_sitter::InputEdit`.
- Kept `parse()` as the full-parse convenience API, backed by the same parser helper as incremental reparsing.
- Added validation that the unchanged old/new prefix and suffix match before converting `MdTextEdit` into a tree-sitter edit.
- Added tests proving incremental reparse matches a full parse after an inserted heading, and rejects mismatched edit/source pairs.
- Updated the `tsift-md-ast` README and changelog.

While running the required repo check, `make check` exposed that the local OpenCode npm package gate used `npm publish --dry-run`, which now fails after `opencode-tsift@0.1.63` has already been published. I switched the local `make check` target to `npm pack --dry-run`; the release workflow still owns the actual publish dry-run before tagged npm publishes.

Verification:

- `cargo test -p tsift-md-ast` passed.
- `make check` passed on the final narrowed diff.
- Latest GitHub Actions CI check was `completed success` for workflow `CI` on `main` at run `27082060444`.
`#trt1inject` is already implemented in the current tsift tree, so I closed it as done rather than adding duplicate code.

Evidence checked:

- `packages/tsift-cli/src/context_pack.rs` builds the context-pack result-set keys and runs the `findings_injection` phase, folding trusted, fresh authored findings into `ContextPackReport.findings` for nodes already in the result set.
- `packages/tsift-cli/src/commands/finding.rs` has `collect_injectable_findings()`, which selects only matching `finding` / `decision` / `note` nodes, excludes `draft`, excludes stale/no-watermark findings by re-resolving the anchor watermark, and fails open to an empty list when `.tsift/findings.db` is absent.
- `VERSIONS.md` records the shipped `#trt1p2` context-pack injection behavior; `#trt1p2b` later extends the same trusted+fresh contract to `search` and `explain`.


- `cargo test -p tsift-cli --test finding context_pack_` passed: 4 tests cover trusted fresh injection plus draft, stale, and out-of-result exclusions.
<!-- agent:boundary:bef03691 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue auto preset="#spec-test-build-install-commit-push" go -->
- ~~do "*Phase 2c (tsift, optional refinement):* an explicit incremental-reparse helper on top of `parse()` for CRDT keystroke edits (tree-sitter-md is already incremental; today consumers re-`parse()`). Say the word if you want it; I didn't queue a churny item."~~
- ~~[#trt1inject]~~
- ~~[#trt1watermark]~~
- [#trt1capture]
- [#trt1project]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority -->
- [ ] [#trt1watermark] Staleness gating on authored-finding freshness via community_graph_watermark: mark findings whose observed_at_unix predates the current watermark as stale in findings/injection output.
- [ ] [#trt1capture] Opt-in/passive capture of findings from agent-doc session archives: parse .agent-doc archives, extract authored finding/decision/note items, upsert via authored_node_projection. Opt-in only.
- [ ] [#trt1project] md/html on-demand projection of the findings graph (graph store stays source of truth); render finding/decision/note nodes + annotates edges grouped by anchor.
- [ ] [#surrealbulk] SurrealDB projection performance (consolidates surrealbulk + surrealbulk-close + surreal-batch-bfs + surreal-fixture): replace per-row upsert loops with bulk/transactional row load + one-pass secondary-index rebuild; add batch frontier expansion; record SurrealDB samples in graph-db-performance-history.json fixture; re-run backend-eval benchmarks, update SPEC.md/BENCHMARKS.md table, measure refresh vs load separately.
- [/] [#e2e-install] E2E verify opencode-tsift install in a fresh opencode project: opencode plugin opencode-tsift && opencode then /tsift-status returns healthy (consolidates e2e-install + full-e2e-verify). Live-verify only; npm publish + OIDC auth already done (opencode-tsift@0.1.63 on npm).
- [/] [#7qky] [recommended] AGENT-DOC-SCOPED (drive from an agent-doc session, not tsift): re-measure Codex redundant runbook reads after the v0.34.0 retain-dont-reread nudge accumulates live Codex sessions; compare to baseline (Claude 11.4% / Codex 12.6% / OpenCode 5.3%). If Codex rate does not drop, build a session-scoped content-dedup LEDGER in a shared LEAF crate (register(id,hash)/should_deliver->Full|Stub; exemptions via hash-change+compaction; session-id from harness). GATED on accumulating post-nudge Codex sessions.<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#rankdefault] Promote ranked_neighborhood into default graph lookup/context-pack/evidence output once quality gates pass; compare ranked vs unranked on real backlog/code queries (useful-hit density, duplicate-name precision, handle coverage, latency, emitted tokens). Also integrate the shipped #memgraphrag1 decay-weighted memory ranking so memory + code share one ranking over the unified graph.
<!-- /agent:review -->
###
<!--
Can we increase max-hop caps?
Could FalkorDB help with performance?
-->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->

