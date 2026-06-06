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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260606-043708.md`*

Compacted content:
- Archived 1 response topic(s): [#trt1p3]
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: [#trt1p4] — Findings Graph Layer Phase 4 (passive harvest) — opus-4-8

> **Queue prompt:**
>
> [#trt1p4]


Shipped Phase 4. **The Findings Graph Layer is now feature-complete (Phases 1–4).** Clean queue drain (head matched the work).

#### Passive harvest — `tsift finding harvest`

Closes the capture loop: extracts `draft` candidate findings from the agent-doc session archives tsift already accumulates (`.agent-doc/archives/*.md`). A line becomes a candidate only when it pairs a decision/insight signal (`decided`, `decision`, `invariant`, `gotcha`, `by design`, `fail-closed`, `source of truth`, …) with an inline-code token that resolves to an **indexed symbol or real file** (a resolvable watermark proves the anchor). `kind` = `decision` for design-intent, else `note`.

- **Fail-closed gate** — bails unless `.tsift/config.toml` sets `[findings] passive_harvest = true` (new `FindingsConfig`, default off). A user who never opts in sees zero captured rows.
- **Safe by construction** — everything lands as `draft` (excluded from injection until promoted), runs are bounded (`HARVEST_CAP`), idempotent (content-derived ids), and never overwrite an existing finding (`skipped_existing`).

#### Promotion — `tsift finding promote <id>`

Flips a `draft` → `trusted` (preserving anchor/watermark/provenance), the precision gate that makes a finding eligible for hot-path injection (Phase 2) and map annotation (Phase 3). Idempotent on trusted; fails closed on unknown id.

#### Verification

- **Tests** — 5 new CLI tests in `tests/finding.rs` (fail-closed gate, draft extraction, idempotency, promotion+idempotent-repromote, unknown-id) + 2 config tests in `tsift-index/src/config.rs`. Full finding suite 18/18.
- **`make check` + clippy** — green on **rustc 1.96.0** (CI-matched toolchain).
- **Live smoke** — `cargo install --path .` (both binaries); confirmed live `tsift finding harvest` gates on config and harvests idempotently.
- **Shipped** — `tsift@6e568b1` (pushed), superproject pointer `agent-loop@d0ce227`. CI monitoring in progress.

#### Queue / backlog

- `#trt1p4` → **done**. Phases 1–4 all shipped.
- Remaining: **`#trt1p2b`** (fold findings into `search`/`explain` — the last hot-path sub-scope) → **`#trt1cfg`** (configure agent-loop to use the layer; its phase-2–4 gate is now satisfied, so it unblocks once p2b lands).

Top of backlog (`#next-steps`): **`#trt1p2b`** — search/explain hot-path injection (same trusted/fresh/result-set contract as context-pack).
<!-- agent:boundary:824d8e36 -->
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
- ~[#trt1p4]~
- [#trt1p2b]
- [#trt1cfg]
- ~Once [#trt1] is complete, configure agent-loop tsift config to use findings graph layer.~
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
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
