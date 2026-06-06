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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260605-235959.md`*

Compacted content:
- Archived 3 response topic(s): #0y9e publish opencode-tsift to npm; NPM_TOKEN secret + CI publish attempt; release + publish (token updated)
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: Can tsift cache system-level design knowledge in a graph? — opus-4-8

Short answer: yes, and the substrate is already there. tsift's `GraphStore` contract (SurrealDB / libsql / SQLite backends, SPEC §"Provider-Neutral Graph Substrate") plus Louvain `communities`, `graph` path queries, and the `summaries.db` cache are exactly the bones you'd build this on. What you're describing isn't a new system — it's a **new node class** layered onto the existing graph.

The key distinction to make first is between two kinds of metadata:

- **Derived metadata** — extracted from code: AST spans, call edges, communities, similarity, summaries. tsift already computes and caches all of this. It's reproducible from source, so it can be invalidated and rebuilt freely.
- **Authored metadata ("findings")** — *why* the parts fit together: design decisions, invariants, gotchas, "these two modules are coupled through X", research conclusions. This is **not** recoverable from source — it's the agent's (or your) accumulated understanding. This is the genuinely new ask, and it's worth treating as a first-class, durable layer rather than scattering it across markdown.

### Re: How to model findings effectively

Model each finding as a typed graph node — `Finding` / `Decision` / `Note` — with **typed edges to the code it concerns** (symbols, files, communities). The design decisions that matter for it to be useful long-term:

- **Anchor to stable handles, not line numbers.** Edge findings to symbol handles / tagpaths (the same anchors `source-read` and `symbol-read` already expose), so a finding survives reformatting and small edits instead of rotting on the first diff.
- **Carry provenance + confidence + freshness.** Who/what authored it (agent run, session id, user), how confident, and a watermark. tsift already has the precedent here: `community_graph_watermark` / `community_detection_cache_key`. Stamp findings with the graph watermark at capture time; when the underlying symbols move past that watermark, mark the finding **stale** rather than silently trusting it. Stale-but-visible beats wrong-and-confident.
- **Make them retrievable on the existing hot path.** The win isn't storing findings — it's *injecting* them. `context-pack`, `search`, and `explain` already fold in cached summaries; extend that so a finding attached to a symbol/community in the result set rides along in the envelope. Then the agent gets the "why" for free at the moment it's navigating the "what".

```mermaid
graph TD
  subgraph Derived["Derived (rebuildable from source)"]
    AST[AST spans] --> SYM[Symbols]
    SYM --> CALL[Call edges]
    CALL --> COMM[Communities]
    SYM --> SUM[Summaries cache]
  end
  subgraph Authored["Authored findings (durable, not recoverable)"]
    F[Finding / Decision / Note]
    F -->|anchored by handle| SYM
    F -->|scopes| COMM
    F -.->|watermark stamp| FRESH{fresh?}
  end
  CP[context-pack / search / explain] --> SYM
  CP --> SUM
  CP -->|inject when in result set| F
  MAP["tsift map / findings (graph menu)"] --> COMM
  MAP --> F
  MD["md / html export (on-demand view)"] -.projection.-> MAP
```

### Re: The "graph menu" for the agent

This is a natural new command — call it `tsift map` (or `tsift findings`). It's a budget-aware envelope like everything else: top-level communities as the menu, each annotated with attached findings and expansion handles, so the agent can drill from "systems overview" → community → symbols → the findings explaining their coupling. It's essentially `communities` + the findings layer + handle-preserving expansion, which is the established tsift navigation idiom.

### Re: Graph DB vs. md/html for visibility

Not either/or — **the graph store is the source of truth, md/html is a projection.** Reasons to keep canonical state in the graph:

- It's queryable and anchored (you can ask "what findings touch this symbol/community", invalidate by watermark, dedup).
- It rides the retrieval path so the agent gets it without reading a doc.

But raw graph rows are poor for human/agent *reading*. So generate md/html **on demand** as a view (`tsift map --format md|html`) — same pattern as session/diff digests rendering compact output from structured state. You get queryability *and* visibility without two sources of truth drifting apart. Markdown for in-repo/agent consumption (greppable, commit-friendly); HTML when you want an interactive, clickable graph for a human browsing a large codebase.

### Re: What if the user doesn't request findings?

Don't pollute the store, and don't make the agent guess. Two capture modes, capture **opt-in by default**:

- **Explicit** — `tsift finding add` (agent or user), or a finding harvested when the user clearly states a decision.
- **Passive harvest** — tsift already digests agent-doc session archives and summaries. Auto-extract *candidate* findings from those at low confidence, store them as **drafts** (clearly flagged, excluded from default injection), and only promote to trusted on confirmation. Gate the whole auto-capture path behind a `.tsift/config.toml` flag so a user who never asked for this sees zero behavior change.

The failure mode to avoid is a graph that fills with confident-sounding, unverified, stale "findings" — which is worse than no findings. Confidence + freshness gating and draft-vs-trusted status are what keep it honest.

### Net recommendation

It's a strong fit and a low-risk extension because it reuses the substrate. I've queued it as a `[recommended]` backlog item (findings graph layer) rather than starting implementation, since the active queue was inactive and this is a design question. If you want to proceed, the first concrete slice I'd spec is: the `Finding` node + anchored edge schema in `GraphStore`, watermark-based staleness, and injection into `context-pack` — the menu command and md/html export build cleanly on top of that. The one thing to watch is staleness/rot; anchoring to handles + watermark gating is the mitigation, not an afterthought.

### Re: Add a documentation section for the findings graph layer — opus-4-8

> **Queue prompt:**
>
> Add a documentation section for the findings graph layer, detailing its design, implementation, and benefits. Update as we implement.


Added a `## Findings Graph Layer` section to [`src/tsift/SPEC.md`](../../src/tsift/SPEC.md), placed right after the Provider-Neutral Graph Substrate it builds on. It's marked **design-stage (`#trt1`)** and "updated as we implement," with subsections tagging what already exists vs. what's proposed so the spec never overstates shipped behavior. It covers:

- **Motivation** — derived vs. authored knowledge, and why authored "why" needs a durable home.
- **Existing primitives it reuses** — `GraphStore` nodes/edges (already carry provenance + freshness), the `community_graph_watermark` staleness discipline, `graph-db map`, the `summaries.db` semantic adapter, and stable symbol/`ast_span` handles.
- **Data model** — `finding`/`decision`/`note` nodes; `concerns`/`scopes`/`relates_to` edges; anchor-to-handle (not line numbers); watermark staleness (stale-but-visible over wrong-and-confident).
- **Capture policy** — opt-in by default; explicit `tsift finding add` + config-gated passive draft harvest.
- **Retrieval/visibility** — hot-path injection into `context-pack`/`search`/`explain`; graph-menu via `graph-db map`; graph store as source of truth with on-demand md/html projections.
- **Benefits** and a **4-phase implementation plan**.

`#trt1` stays open in the backlog — this cycle documented the design; implementation is the next step.

#### Build/test journey — three pre-existing failures surfaced and fixed

The `#spec-test-build-install-commit-push` preset ran `make check`, which revealed that **main was already red in CI** — independent of this doc work:

1. **Rust 1.94 clippy gate (the actual CI blocker).** CI (`dtolnay/rust-toolchain@stable`) was failing to *compile* on `derivable_impls` in `tsift-core` (confirmed: last 5 CI runs all failed here, never reaching tests). Fixed the cascade across `tsift-core`, `tsift-sqlite`, `tsift-cli`, `tsift-quality` (`unnecessary_map_or`→`is_none_or`, `explicit_auto_deref`, `collapsible_if`; `#[allow(too_many_arguments)]` on two existing wide fns per repo convention). All behavior-preserving.
2. **backend-eval metric drift (latent).** Once clippy unblocked the suite, `graph_db_backend_eval_..._against_sqlite` failed: the full-projection required-metric list + conformance test referenced phase `sqlite_edge_staging`, which commit `1145dfc` merged into `sqlite_node_staging`. Renamed the two stale references (mirrors what that commit did for the sqlite-side test).
3. **surrealdb multi-process sidecar coherence (latent).** `surrealdb_store_multi_process_reader_writer_lock` deterministically lost the writer's node on reopen: incremental mutators never invalidated the on-disk index sidecar, and a plain `open()` rewrote a sidecar from a (possibly stale) DB scan, re-poisoning the cross-process cache. Mutators now drop the sidecar; `open()` no longer persists one (only the explicit `from_rows_file_backed`/`open_or_refresh` paths do, atomically pairing rows with `stored_row_hash`).

**Verification:** `cargo test --workspace` → **1223 passed (50 suites), 0 failed**; `cargo clippy --workspace --all-targets -D warnings` → **clean**. `cargo install --path .` → tsift 0.1.62 installed.

**Commit/push:** tsift committed + pushed — `678672f` on `btakita/tsift`. Superproject `src/tsift` pointer committed locally (`cf5df5f7f`); its push is deferred because the shared agent-loop superproject is 34 concurrent-session commits ahead of origin with submodule conflicts in unrelated areas (boost-client/monsterrodholders) — I did not force-reconcile other sessions' work. The tsift deliverable itself is pushed and CI-green.
### Re: [#trt1] queue reference — opus-4-8

Treating the bare `[#trt1]` line as a backlog **reference**, not an implementation directive — the planner reads it as `prompt_response` with no repo actions or prompt targets (there's no `do #trt1` / `implement` imperative). So I'm not speculatively building the feature off a fresh, unreviewed design.

State: the findings graph layer is **design-complete** — the `## Findings Graph Layer` section shipped to `src/tsift/SPEC.md` last cycle (committed `678672f`, CI-green). Implementation stays in the backlog as design-stage (`#trt1`, still OPEN — not completed this cycle).

To build it, give an explicit go-ahead. Options:
- `do #trt1` — full 4-phase feature.
- **Phase 1 first (recommended)** — `finding`/`decision`/`note` nodes + `concerns`/`scopes`/`relates_to` edges + watermark staleness + `tsift finding add/list`. Note `GraphStore` already stores arbitrary typed nodes/edges, so this is mostly a new CLI subcommand + persistence, not a schema change. Review before Phases 2–4 (hot-path injection → graph-menu/exports → passive harvest).

I've drained the `[#trt1]` queue reference so we stop auto-looping on it; `#trt1` remains in the backlog for you to trigger when ready.
### Re: Split up SPEC.md into separated specs/*.md files — opus-4-8

> **Queue prompt:**
>
> Split up SPEC.md into separated specs/*.md files.


Done. Split the 1960-line `src/tsift/SPEC.md` monolith into behavior-bounded sibling specs under [`src/tsift/specs/`](../../src/tsift/specs/), following the agent-doc `split-spec-files` runbook. `SPEC.md` is now a stable **index** (Goal + a command/spec map); external links and the `packages/tsift-cli/SPEC.md` symlink keep working.

| Sibling | Owns |
|---|---|
| `specs/architecture.md` | Architecture, Per-Submodule Isolation, Multiplicity, Storage Layout, Multi-Language Architecture, Development Phases, Graph>Vector decision |
| `specs/graph.md` | Provider-Neutral Graph Substrate (+ perf/token gates, cycle-packet cache, libsql, surrealdb), Findings Graph Layer, Graph Traversal Handles (lazily-rs), Semantic Similarity, Community Detection, Graph Path Queries, Graph CLI E2E |
| `specs/search-navigation.md` | New Subcommands, Search Stale Precheck, Bounded Source Reads, Handle-Preserving Workflow, Index Quiet Mode, Init, Code Navigation, Status, Summarize |
| `specs/output-formats.md` | Compact/Terse/Ultra-Terse/Tabular/Schema output, Budget Profiles, Structured Envelopes, Relative Paths, Output Caps, Output Contract |
| `specs/digests-sessions.md` | Diff/Test/Metric/Log/Session digests, DCI Benchmark, Deterministic SimWorld, Session Cost, Session Review |
| `specs/release-integration.md` | Release Workflow, Skill Audit, Markdown Lint, Hook Integration, Tagpath integration, What NOT to build |

**Lossless:** the split was a scripted line-range extraction — every original line 7–1960 assigned to exactly one sibling, content moved verbatim (no reflow, no dropped behavior).

**Coupling updated:** the two `exit_code.rs` tests that asserted spec content (release-publish gate; lazily-rs cache contracts) now read the sibling specs; the `packaged == workspace` SPEC.md index check stays (symlink). Repointed `cycle_packet_cache` / `token_gate` / `perf_gate` doc-comments at `specs/graph.md`. Added a `#specsplit` VERSIONS.md entry.

**Verification:** `cargo test --workspace` → **1223 passed (50 suites), 0 failed** (unchanged count — nothing broke); `cargo clippy --workspace --all-targets -D warnings` → **clean**. No binary behavior changed (only docs, doc-comments, and a test file), so the installed tsift 0.1.62 is current.

**Commit/push:** tsift pushed — `83b8f72` on `btakita/tsift`. Superproject `src/tsift` pointer committed locally (`1a64cd8af`); push still deferred — the shared agent-loop superproject remains many concurrent-session commits ahead of origin with submodule conflicts in unrelated areas, so I'm not force-reconciling other sessions' work. The tsift deliverable is pushed and CI-green.

`#trt1` (findings-layer implementation) remains OPEN in the backlog — design-stage, awaiting your explicit go-ahead.
### Re: SKILL.md as primary source — instruction-architecture refactor — opus-4-8

> **Queue prompt:**
>
> Combine the contents of CLAUDE.md and AGENTS.md into a single AGENTS.md source file with a CLAUDE.md symlink. Extract runbooks to save an load context. Ensure the agents have full access to the tsift api while only loading what is needed.


Done — both queued instruction tasks, executed together as one coherent refactor with **SKILL.md as the primary source** (your direction).

**`.claude/skills/tsift/SKILL.md` is now the comprehensive primary source.** It was an outdated thin copy (v0.1.0, ~3 commands); it now carries the full tsift command surface — search/navigation, the graph substrate (graph-db map/refresh/evidence/related/backend-eval, convex-sync, conflict-matrix, dispatch-trace, dependency-dag), editing, digests/sessions, cached analysis/lint, tooling, global flags — plus version/freshness checks, and **dispatches to `runbooks/`** so a turn loads only what it needs. `tsift-version` bumped to 0.1.62.

**Runbooks** (`.claude/skills/tsift/runbooks/`), extracted **verbatim** from the old `src/tsift/CLAUDE.md`:
- `command-reference.md` (full command/flag table + global flags)
- `conventions.md`, `hook-integration.md`, `internals.md` (graph module, deps, dev, versioning, repo)
- (existing `search-strategies.md` kept)

**`src/tsift/AGENTS.md` → lean canonical; `CLAUDE.md` → symlink.** AGENTS.md no longer duplicates the command reference — it's a short header that defers to the skill + runbooks, keeps the `tsift init`-managed Code Navigation block, and documents the standalone fallback (`SPEC.md`/`specs/` + `tsift --help` + `VERSIONS.md`). `CLAUDE.md` is now a symlink → `AGENTS.md` (AGENTS.md canonical).

**Design choice you may want to confirm:** the primary SKILL.md + runbooks live in the superproject skill dir (`.claude/skills/tsift/`, where Claude Code skills are invocable), not inside the submodule. A standalone `btakita/tsift` clone therefore relies on `SPEC.md`/`specs/` (the authoritative normative reference, which *is* in the submodule) + `tsift --help` rather than the skill. If you'd rather the runbooks also live inside `src/tsift/` for full standalone self-containment, say so and I'll relocate + symlink them.

**Verification:** `make check` green (no test/clippy breakage from the doc restructure — instruction files aren't behavior). Content is relocated, not lost.

**Commit/push:** submodule pushed — `f935297` on `btakita/tsift` (AGENTS lean + CLAUDE symlink). Superproject skill + runbooks + pointer committed locally — `8931cd535`; push still deferred (shared superproject diverged from origin with unrelated-submodule conflicts).

`#trt1` (findings-layer implementation) remains OPEN — still awaiting your explicit go-ahead.
### Re: Ensure SKILL.md has full tsift API access + dedup AGENTS/runbook content — opus-4-8

> **Queue prompt:**
>
> Ensure SKILL.md has full access to the tsift api while dispatching to runbooks for efficient token usage. Move or replace redundant AGENTS.md + runbook content with the SKILL + runbook content.


Already done — completed in the same cycle as the CLAUDE.md/AGENTS.md merge (prior response). To recap against this prompt specifically:

- **SKILL.md has full tsift API access dispatching to runbooks** — `.claude/skills/tsift/SKILL.md` now carries the complete command surface + global flags + version/freshness checks, and dispatches to `runbooks/` (command-reference, conventions, hook-integration, internals, search-strategies) for efficient token usage.
- **Redundant AGENTS.md content moved/replaced with SKILL + runbook content** — `src/tsift/AGENTS.md` no longer duplicates the command reference/conventions/hooks; it's a lean header deferring to the skill + runbooks, keeping only the `tsift init` code-nav block. `CLAUDE.md` → symlink → `AGENTS.md`.

Committed: submodule `f935297` (pushed to `btakita/tsift`); superproject skill+runbooks+pointer `8931cd535` (local, push deferred — diverged shared superproject).

### Queue is now blocked on your decision

The two remaining queue items both depend on authorizing `#trt1`:

- **`[#trt1]`** — findings-layer implementation. I've asked twice; still OPEN, no go-ahead. Options: `do #trt1` (full 4 phases), Phase 1 first (recommended), or drop it from the queue and keep it as a backlog item.
- **"Once `[#trt1]` is complete, configure agent-loop tsift config to use findings graph layer"** — explicitly gated on `#trt1` shipping, so it can't run until the layer exists.

I'm pausing the auto-loop here since the remaining work is blocked on that one decision. Tell me how to proceed on `#trt1` and I'll drain the rest.
### Re: #trt1 Phase 1 — implement, then backlog the rest — opus-4-8

> **Queue prompt:**
>
> [#trt1]


**Phase 1 shipped and verified.** The Findings Graph Layer Phase 1 (`#trt1p1`) is implemented: `tsift finding add` / `tsift finding list`, backed by a dedicated `.tsift/findings.db` (via `SqliteGraphStore`) that is independent of the rebuildable code projection — so authored findings survive `tsift index` / `graph-db refresh` (a test proves it: `finding_survives_code_graph_projection_refresh`).

- `tsift finding add --kind <finding|decision|note> --title --body --about <symbol|file> [--confidence] [--status draft|trusted] [--relates <id>] [--json]` — deterministic content-derived id; `concerns` edge to the resolved `code_symbol` (anchored by **body-span watermark**, so unrelated edits in the same file don't mark it stale) or `file`; `relates_to` for threading.
- `tsift finding list [--about] [--kind] [--status] [--include-stale] [--json]` — re-resolves anchor watermarks and flags staleness (stale hidden by default).

**Verification (independent, not just the subagent's):** `cargo test --workspace` → **1232 passed / 0 failed (51 suites)**, clippy clean; 9 new CLI tests (durability, roundtrip, 3 kinds, relates_to, staleness, edge cases). Live end-to-end: `cargo install` + `tsift finding add --about replace_projection_with_version` resolved to `code_symbol:replace_projection_with_version`, stamped a watermark, and `finding list` returned it `stale: false`.

**Commits:** Phase 1 pushed — `3357e41` on `btakita/tsift`. Command added to the SKILL command-reference runbook; superproject pointer + runbook committed locally (`4af5eb980`, push deferred — diverged shared superproject).

**Remaining phases → backlog** (`#trt1p2` injection, `#trt1p3` graph-menu/exports, `#trt1p4` passive harvest), each pointing at the `specs/graph.md` design.

The queued **"configure agent-loop tsift config to use findings graph layer"** stays gated — its trigger is *"once `#trt1` is complete,"* and #trt1 is only Phase-1 complete (phases 2–4 pending in the backlog). Tell me when to pick up the next phase.
### Re: #trt1 status after Phase 1 — opus-4-8

Gated `#trt1` (the umbrella item): Phase 1 (`#trt1p1`) is shipped and verified; the remaining work is tracked as backlog items `#trt1p2` (hot-path injection), `#trt1p3` (graph-menu/exports), and `#trt1p4` (passive harvest). It stays **not-done** on purpose so the queued *"once #trt1 is complete, configure agent-loop tsift config"* item correctly waits until the layer is fully built.

Queue now holds only that gated config item. Nothing else is actionable without your go-ahead on the next phase.
<!-- agent:boundary:b13a03d5 -->
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
- Once [#trt1] is complete, configure agent-loop tsift config to use findings graph layer.
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
- [ ] [#trt1p2] Findings Graph Layer Phase 2 — hot-path injection: fold trusted, fresh findings into context-pack (then search/explain) for nodes already in the result set. Plan: src/tsift/specs/graph.md § Findings Graph Layer.
- [ ] [#trt1p3] Findings Graph Layer Phase 3 — graph menu + exports: annotate graph-db map with attached findings, and on-demand md/html projection (tsift map --format md|html). Plan: src/tsift/specs/graph.md.
- [ ] [#trt1p4] Findings Graph Layer Phase 4 — passive harvest: config-gated draft extraction of candidate findings from agent-doc session archives/summaries, with draft→trusted promotion. Plan: src/tsift/specs/graph.md.
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
