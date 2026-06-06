---
agent_doc_session: tsift-v0.1
resume: 019e85d7-f9ee-79f3-b597-4ed352495bbd
agent: claude
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
queue_active: true
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260606-041407.md`*

Compacted content:
- Archived 1 response topic(s): [#trt1cfg]
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: [#trt1p3] — Findings Graph Layer Phase 3 (graph menu + exports) — opus-4-8

> **Queue prompt:**
>
> [#trt1p3]


Shipped Phase 3. The queue head matched the work this cycle (clean drain, no divergence).

#### Map findings annotation

`tsift graph-db map` now annotates each **community, hub, and `--focus` node** with the trusted, fresh findings that `concern` its displayed members / label / symbol. Reuses the Phase 2 `collect_injectable_findings` contract, so `draft`, stale (anchor past watermark), and no-watermark findings never appear. Matching is bounded to displayed names (not a full-store scan) and deduped by finding id per node. The JSON `overview` communities/hubs and the `focus` report each gain an omit-when-empty `findings` array (`id`, `kind`, `title`, `about`, `anchor_kind`, optional `confidence`); fails open to none when `findings.db` is absent.

#### On-demand md/html projection

New `tsift graph-db map --format md|html` renders the same overview + findings:
- **`md`** — greppable / commit-friendly; one `📌 <kind>: <title> (about <anchor>)` bullet under each annotated node.
- **`html`** — self-contained, escaped, styled page for an interactive human view.

Both are pure projections — the graph store / JSON stays the single source of truth, same pattern session/diff digests use. Without `--format`, the command keeps its JSON/text output.

#### Verification

- **Tests** — 3 new CLI tests in `packages/tsift-cli/tests/graph_db_conformance.rs` (hub annotation, md+html rendering, draft exclusion), all green.
- **`make check` + clippy** — full suite green on **rustc 1.96.0** (matched CI's rolled toolchain this time; clippy `-D warnings` clean — no repeat of the 1.94/1.96 drift that bit last cycle).
- **Live smoke** — `cargo install --path .` (both binaries); confirmed the live `tsift graph-db map --format md` emits the 📌 finding lines.
- **Shipped** — `tsift@e054da0` (pushed), superproject pointer `agent-loop@044f960`. CI monitoring in progress.

#### Queue / backlog

- `#trt1p3` → **done**.
- Remaining, in order: **`#trt1p4`** (passive harvest) → `#trt1p2b` (search/explain injection) → `#trt1cfg` (configure agent-loop, still blocked until p4 + p2b ship).

Top of backlog (`#next-steps`): **`#trt1p4`** — Phase 4 (config-gated passive harvest of draft findings from agent-doc session archives, with draft→trusted promotion).
<!-- agent:boundary:959161ac -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- ~[#trt1]~
- ~[#trt1p2]~
- ~[#trt1p3]~
- [#trt1p4]
- [#trt1p2b]
- [#trt1cfg]
- ~Once [#trt1] is complete, configure agent-loop tsift config to use findings graph layer.~
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#trt1p4] Findings Graph Layer Phase 4 — passive harvest: config-gated draft extraction of candidate findings from agent-doc session archives/summaries, with draft→trusted promotion. Plan: src/tsift/specs/graph.md.
- [ ] [#trt1p2b] Findings Graph Layer Phase 2b — fold trusted, fresh findings into `search` and `explain` output, reusing the trusted/fresh/result-set injection contract from context-pack (#trt1p2). Plan: src/tsift/specs/graph.md § Hot-path injection (Phase 2).
- [ ] [#trt1cfg] After #trt1 phases 2-4 ship (findings hot-path injection), configure agent-loop's tsift config to use the findings graph layer. Gated on #trt1 completion.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#trt1] [recommended] Findings graph layer: add authored Finding/Decision/Note nodes to GraphStore, anchored by stable symbol-handle/tagpath edges (not line numbers), with provenance+confidence+freshness fields gated on community_graph_watermark for staleness; expose retrieval via context-pack/search injection and a / graph-menu command; opt-in/passive capture from agent-doc session archives; md/html as on-demand projection, graph store as source of truth. Spec in src/tsift/SPEC.md.
- [/] [#0y9e] I set NPM_TOKEN="$(passage btak/NPM_TOKEN)". Publish to npm.
- [/] [#astnav-npm] Configure NPM_TOKEN + TSIFT_ENABLE_NPM_PUBLISH secrets, publish opencode-tsift to npm (medium)
- [/] [#surreal-batch-bfs] #surreal-batch-bfs-gated: Batch frontier expansion - gated (low)
- [/] [#surreal-fixture] Record SurrealDB samples in graph-db-performance-history.json fixture (medium)
- [/] [#surrealbulk-close] Close #surrealbulk review item: re-run benchmarks, update SPEC.md benchmark table, mark done
- [/] [#rankdefault] Promote ranked_neighborhood into default graph lookup/context-pack/evidence output only after quality gates pass; compare ranked vs unranked output on real backlog/code queries for useful-hit density, duplicate-name precision, handle coverage, latency, and emitted tokens.
- [/] [#surrealbulk] Batch SurrealDB projection load: replace per-row upsert loops with bulk/transactional row load plus one-pass secondary-index rebuild; measure refresh/load separately.
- [/] [#full-e2e-verify] In fresh project: opencode plugin opencode-tsift && opencode then /tsift-status. On success mark #e2e-install done.
- [/] [#npm-publish-verify] After auth configured, tag release and verify publish-opencode-plugin CI job succeeds and opencode-tsift appears on npmjs.org
- [/] [#npm-auth-setup] Configure NPM_TOKEN secret + TSIFT_ENABLE_NPM_PUBLISH=true variable in GitHub repo settings
- [/] [#e2e-install] Verify end-to-end install: fresh opencode project + opencode plugin opencode-tsift + /tsift-status
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
