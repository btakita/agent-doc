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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260606-033119.md`*

Compacted content:
- Archived 13 response topic(s): Can tsift cache system-level design knowledge in a graph?; How to model findings effectively; The "graph menu" for the agent; 10 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: [#trt1cfg] — configure agent-loop to use the findings layer — opus-4-8 (HEAD)

`#trt1cfg` is the queue head, but it is **blocked**: it is gated on Findings Graph Layer phases 2–4 shipping, and only Phase 1 (`#trt1p1`) was committed. Those phases also sit *after* `#trt1cfg` in the queue — a dependency-ordering inversion. So this cycle I did the real next unblockable work, **Phase 2 (`#trt1p2`) hot-path injection**, and moved `#trt1cfg` to the end of the backlog so it drains after its dependencies.

#### Phase 2 shipped — `context-pack` findings injection

Trusted, fresh findings now ride the existing agent hot path instead of needing a separate `tsift finding list` call. `context-pack` gains a `findings` section:

- **Result-set scoped** — a finding is injected only when its `about` anchor matches a node already in the result set: touched files/symbols, diff-preview files + their touched symbols, and the exploration packet's source-window files + relationship endpoints. Never a blanket store dump.
- **Trusted + fresh only** — `draft` findings, stale findings (anchor advanced past the captured watermark), and no-watermark findings are excluded, so confident-but-wrong authored context can't reach the hot path.
- **Cheap + safe** — capped at the preview-item budget, body-truncated, omitted entirely when empty, and fails open to empty when `findings.db` is absent (zero envelope change for projects with no authored knowledge). Each preview carries a stable `handle`, `id`, `kind`, `title`, `about`, `anchor_kind`, optional `confidence`, and an `expand` command back to the full anchored set.

New `collect_injectable_findings` helper in `commands/finding.rs`; wiring + `ContextPackFindingPreview` in `context_pack.rs`; spec in `specs/graph.md` § Hot-path injection (Phase 2); `VERSIONS.md` updated.

#### Verification

- **Tests** — 4 new CLI tests in `packages/tsift-cli/tests/finding.rs` (injection, draft-exclusion, stale-exclusion, result-set scoping); `finding` suite 13/13 green.
- **`make check`** — full suite green (exit 0; scanned the log for `result: FAILED`/errors per the RTK exit-masking caveat — none).
- **Live smoke** — `cargo install --path .` (both `tsift` and `tsift-cli`); a real fixture confirms the `findings` section appears in the live `context-pack --json` envelope.
- **CI** — pushed `tsift@ed10268`; CI run `27051669940` in progress at finalize. Local `make check` mirrors the CI suite and is green.
- Committed + pushed: `tsift@ed10268`; superproject pointer bump `agent-loop@443de61`.

#### Queue / backlog

- `#trt1p2` → **done**.
- Added `#trt1p2b` — the remaining sub-scope of hot-path injection: fold findings into `search`/`explain` (same contract).
- Reordered the backlog into true dependency order so the blocked config item lands last: **`#trt1p3` → `#trt1p4` → `#trt1p2b` → `#trt1cfg`**.
- `#trt1cfg` stays open/blocked until p3, p4, and p2b ship.

Top of backlog (`#next-steps`): **`#trt1p3`** — Phase 3 (graph menu + md/html exports).
<!-- agent:boundary:9dc496f7 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue auto preset="#spec-test-build-install-commit-push" go -->
- ~Combine the contents of CLAUDE.md and AGENTS.md into a single AGENTS.md source file with a CLAUDE.md symlink. Extract runbooks to save an load context. Ensure the agents have full access to the tsift api while only loading what is needed.~
- ~Ensure SKILL.md has full access to the tsift api while dispatching to runbooks for efficient token usage. Move or replace redundant AGENTS.md + runbook content with the SKILL + runbook content.~
- ~[#trt1]~
- ~[#trt1p2]~
- [#trt1p3]
- [#trt1p4]
- [#trt1p2b]
- [#trt1cfg]
- ~Once [#trt1] is complete, configure agent-loop tsift config to use findings graph layer.~
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#trt1p3] Findings Graph Layer Phase 3 — graph menu + exports: annotate graph-db map with attached findings, and on-demand md/html projection (tsift map --format md|html). Plan: src/tsift/specs/graph.md.
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
