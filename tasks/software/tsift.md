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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260530-184920.md`*

Compacted content:
- Archived 2 response topic(s): monorepo and multiplicity next steps; queued multiplicity implementation
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ Read file:///home/brian/.claude/usage-data/report-2026-05-30-112308.html. What are #next-steps for tsift to save tokens and improve effectiveness of graph/neighborhood queries?

Please analyze claude-mem. Should we drop it or have our own?

### Re: token savings, graph neighborhoods, and claude-mem — gpt-5

I read the Claude Code Insights report and inspected the local `claude-mem` installation, database, logs, and HTTP API. The report is pointing at a real tsift opportunity: not generic “more memory,” but budgeted memory and graph retrieval that prevents large observer prompts from ever being constructed.

Evidence:
- Report scope: 3,888 messages across 317 sessions from May 28-30, with observer/memory sessions called out as a major part of the workflow and a major source of failure.
- Report friction: `claude-mem` observer sessions repeatedly hit `Prompt is too long`, sometimes producing zero usable memory output.
- Local `claude-mem`: plugin `10.5.2` is running as a Bun worker with Chroma MCP; settings currently keep `CLAUDE_MEM_CONTEXT_OBSERVATIONS=50`, `CLAUDE_MEM_CONTEXT_SESSION_COUNT=10`, and terminal output enabled.
- Local data: `claude-mem.db` contains 32,337 observations and 7,400 summaries, so the data is valuable. But `pending_messages` has 6,434 failed rows, 74 pending rows, and 1 processing row.
- Local logs: May 28-30 logs include 223 context-overflow errors and 228 `Prompt is too long` lines.
- tsift’s own `session-review --next-context` for this document flags the same class of waste: largest prompt turn 477,500 tokens, cached input ratio 98.58% across roughly 548M prompt tokens, restart churn, and repeated no-op closeouts.
- The `claude-mem` HTTP API works, but its retrieval is too shallow for this failure class: searching observations for `prompt too long` returned unrelated agent-doc observations instead of the observer overflow failures.

Recommendation: do not hard-drop `claude-mem` today. Keep it as a capture/UI/history source because it has a large existing corpus and a working local API. But tsift should own the durable memory contract: budgeted handoff construction, graph projection, ranked retrieval, evidence packets, and persistence verification. Treat `claude-mem` as a provider/importer until it proves reliable under explicit token budgets.

I added the follow-up backlog in priority order. Top item: `tsift-memory-budget-guard`.

Priority order:
1. `tsift-memory-budget-guard` — prevent observer/memory prompts from exceeding budget before model calls.
2. `tsift-claude-mem-adapter` — import existing claude-mem memory into tsift graph reads without taking over capture immediately.
3. `tsift-neighborhood-token-budget` — make `graph-db related` / `neighborhood` / `evidence` optimize for useful tokens, not just node count and depth.
4. `tsift-memory-eval-harness` — compare claude-mem and tsift retrieval on this actual failure class.
5. `tsift-summary-extract-gate` — make missing summaries an explicit semantic-graph readiness gate.

Changed paths: session document/backlog only; no source edits.

Commands used: `tsift status`, `tsift status --fix`, `agent-doc preflight`, `agent-doc orchestrate --from-exchange --plan`, `agent-doc plan`, `agent-doc read`, `lynx -dump` on the report, targeted `sqlite3` queries against `claude-mem.db`, local `curl` calls to the `claude-mem` API, targeted `tsift` searches/source reads, `tsift --envelope session-review --next-context --budget normal`, and `tsift --envelope context-pack --budget normal`.

Verification: document-analysis cycle only; source verification was not required because no code changed. The key checks were the usage-report read, local database/log/API inspection, and tsift’s own next-context/context-pack output.

Confidence: high on the direction. The only caveat is that the current active `claude-mem` worker is a minified plugin cache, so implementation should target either its stable HTTP/SQLite surfaces or a tsift-owned adapter rather than patching cached plugin internals directly.
❯ Should we make our own memory crate/plugin, optimized for our tsift/agent-doc stack? We can import claude-mem into the memory crate db.

### Re: first-party memory crate/plugin — gpt-5

Yes: make our own memory crate/plugin, but do it as a staged replacement, not a hard fork or immediate rip-out of `claude-mem`.

The reason is ownership boundary, not NIH. `claude-mem` is useful as a capture/UI/history source and it already has valuable local data. But the failure mode we care about is inside the contract: unbudgeted observer prompts, oversized raw tool payloads, shallow retrieval over the wrong evidence, and no hard proof that an observation was persisted before the session ends. Those are core tsift/agent-doc concerns, so tsift should own them.

Recommended shape:

1. First-party crate: `tsift-memory`.
   Own a stable schema for observations, session summaries, prompts, artifacts, tool spans, token accounting, embeddings, source handles, and graph node ids. Make importers adapters, not the canonical model.

2. First-party plugin/hook layer.
   The plugin should emit bounded events into `tsift-memory`: prompt target, tool call, tool result artifact handle, response summary, closeout proof, commit hash, session-check result. It should never send raw transcript blobs to a model before a budget pass.

3. Import `claude-mem`, do not depend on it.
   Build a read-only importer for `~/.claude-mem/claude-mem.db` plus Chroma references. Preserve observation ids, session ids, timestamps, project, type, title/facts/narrative fields, and then map them into tsift graph nodes. This gives us migration value without inheriting the runtime failure mode.

4. Keep `claude-mem` running during migration.
   Use it as a comparator and fallback UI while `tsift-memory` proves reliability. Once the eval harness shows lower prompt size, better useful-hit rate, and zero silent memory-output failures, then we can decide whether to disable `claude-mem` capture.

5. Optimize for graph usefulness, not memory volume.
   The memory crate should serve `graph-db related`, `evidence`, `context-pack`, and `session-review` with token-capped ranked packets. The unit of retrieval should be an evidence packet with source handles and expand commands, not a list of verbose observations.

I added one top-level follow-up item for the first-party memory crate/plugin spike. The previously added memory-budget guard, claude-mem adapter, neighborhood token budget, eval harness, and summary gate are the right implementation phases under that umbrella.

Changed paths: session document/backlog only; no source edits.

Commands used: `tsift status`, `tsift status --fix`, `agent-doc preflight`, and `agent-doc plan`.

Verification: document-analysis cycle only; no source verification required.

Confidence: high. The important constraint is to keep `claude-mem` as an import source until tsift proves the new pipeline with measured retrieval and persistence results.

### Re: tsift-memory crate/plugin — gpt-5

Implemented `#tsift-memory-crate-plugin` as a first-party tsift memory spike and marked the backlog item done.

Changed paths:
- `src/tsift` commit `191e76b9703b4ba951bf0599ef9041cec7e1d32f` (`feat: add first-party tsift memory crate`), pushed to `btakita/tsift main`.
- Root superproject commit `11231e961` (`chore: update tsift submodule`), pushed to `btakita/agent-loop main`.
- Main touched areas inside `src/tsift`: `packages/tsift-memory/`, `packages/tsift-cli/src/commands/memory.rs`, `packages/tsift-cli/src/cli.rs`, `packages/tsift-index/src/init.rs`, `packages/opencode-tsift/commands/tsift-memory-status.md`, `SPEC.md`, root reexports, and lock/workspace metadata.

What changed:
- Added a new `tsift-memory` crate with `.tsift/memory.db` schema versioning for memory events, summaries, artifacts, tool spans, embeddings, graph links, and import runs.
- Added token-budgeted handoff planning so oversized memory/tool artifacts are split before model calls instead of becoming prompt-overflow failures.
- Added agent-doc hook contract primitives for prompt targets, tool artifacts, response summaries, closeout proof, and session-check events.
- Added provider-neutral graph projection primitives for `memory_session` and `memory_event` nodes.
- Added a real `claude-mem` SQLite migration surface: `inspect_claude_mem` plus dry-run/apply import from `observations`, `session_summaries`, and `user_prompts`, while reporting adjacent Chroma availability.
- Added CLI surfaces: `tsift memory status`, `init`, `import-claude-mem`, `handoff-plan`, and `query-plan`.
- Added an OpenCode plugin shortcut `/tsift-memory-status` and kept `tsift init --opencode` and the npm package command set in sync.
- Updated `SPEC.md` and root reexports so `tsift::memory` is part of the public package shape.

Commands and verification:
- `tsift rewrite --run 'cargo check --workspace'` passed.
- `tsift rewrite --run 'cargo test -p tsift-memory -p tsift-cli -p tsift-index'` passed.
- `cargo run -- memory status . --json` passed and detected the local `claude-mem` tables plus Chroma directory.
- `cargo run -- memory handoff-plan 'short memory event' --budget-tokens 1000 --json` passed.
- `cargo run -- memory import-claude-mem . --limit 2 --json` passed as a dry-run and planned six events.
- `tsift rewrite --run 'cargo clippy --workspace --all-targets -- -D warnings'` passed.
- `tsift --envelope __digest-runner --kind log --path . --shell-command 'make check'` passed. The digest flagged one expected-looking test-fixture `opening index db` line, but the command exit code was 0 and reported test groups passed.
- `tsift rewrite --run 'cargo install --path . --force'` passed.
- Installed `tsift memory status . --json` passed.
- Latest GitHub Actions CI for `btakita/tsift` was successful: run `26691925937`, jobs `clippy + test` and `live Convex graph acceptance`. There is no separate `tmux-test` workflow/job in this repo.

Escalation: none.

Confidence: high for the spike boundary. This establishes the owned schema/API/CLI/plugin contract and a working `claude-mem` migration path; the remaining queue items should build the stricter budget guard, graph-db adapter, ranked neighborhood budgets, eval harness, and summary readiness gate on top of this substrate.

### Re: do [#tsift-memory-budget-guard] — gpt-5

Completed `#tsift-memory-budget-guard`.

Implemented a fail-closed `tsift memory budget-guard` path in `src/tsift` so raw transcript/session/tool/log payloads are measured before handoff, blocked when they exceed the available prompt budget, and replaced with bounded retry plans plus digest/context commands. The guard now emits `blocked_split_required` reports with replacement strategies, source refs, chunk byte ranges for file retries, and safer inline placeholders for text retries.

Changed and pushed:

- `src/tsift` commit `7778af44987c1b7ac4d0a65bbd545bdcfe6e37b4` (`feat: add tsift memory budget guard`)
- root submodule pointer commit `66ded9204` (`chore: update tsift submodule`)

Key touched surfaces:

- `packages/tsift-memory/src/lib.rs` and README: budget guard contract, report/replacement/chunk structures, token estimation, payload classification, split checks.
- `packages/tsift-cli/src/commands/memory.rs` and parser tests: `tsift memory budget-guard` with `--text`, `--file`, `--byte-start`, `--byte-end`, `--source-ref`, `--payload-kind`, budget/reserve/chunk knobs, and JSON output.
- `packages/opencode-tsift/commands/tsift-memory-guard.md`, installer defaults, and npm package tests: added the Opencode command surface.
- `SPEC.md`: documented the new contract and fail-closed behavior.

Verification passed:

- `cargo fmt`
- `tsift rewrite --run 'cargo test -p tsift-memory -p tsift-cli -p tsift-index'` (`tart-e0ac6695b1`)
- CLI smoke for a small ready payload and an over-budget file payload with byte-range retry chunks
- `tsift rewrite --run 'cargo clippy --workspace --all-targets -- -D warnings'` (`tart-799ab74940`)
- `tsift --envelope __digest-runner --kind log --path . --shell-command 'make check'` (`tart-f0f371023c`)
- `tsift rewrite --run 'cargo install --path . --force'` (`tart-250d7d9395`)
- installed CLI smoke for `tsift memory budget-guard --text 'short payload' --budget-tokens 1000 --json`
- `git diff --check`
- GitHub Actions `main CI` run `26692805969` passed both `live Convex graph acceptance` and `clippy + test`

CI annotations were non-blocking: GitHub's Node.js 20 action deprecation notice and the existing live Convex graph no-op annotation for missing `TSIFT_LIVE_CONVEX_GRAPH_URL`. I did not find a separate tmux-test workflow for this repo, so the CI evidence is the `main CI` run above. No escalation was required.

Confidence: high.

### Re: do [#tsift-claude-mem-adapter] — gpt-5

Completed `#tsift-claude-mem-adapter`.

Implemented a read-only `claude-mem` graph-db adapter in `src/tsift` so graph refresh can project existing `claude-mem` SQLite memory without importing or owning capture. The adapter reads project-local `.claude-mem/claude-mem.db` first, otherwise the default `~/.claude-mem/claude-mem.db`, filters global rows to the current project root, and materializes matching observations, session summaries, user prompts, and Chroma handles as stable graph rows.

Changed and pushed:

- `src/tsift` commit `4a226520acdeb12e1fe6ea6c87209a5d988c736e` (`feat: project claude-mem into graph-db`)
- root submodule pointer commit `ce1c0e0ecde6f41918671ca4812deb88dcf15823` (`chore: update tsift submodule`)

Key touched surfaces:

- `packages/tsift-memory/src/lib.rs`: added `ClaudeMemReadReport` and `read_claude_mem_events`, then reused that reader for import planning/apply so read-only projection and migration share one SQLite read path.
- `packages/tsift-cli/src/lib.rs`: graph-db projection now appends `claude-mem` `session`, `source_handle`, `semantic_concept`, and `semantic_vector_handle` rows with stable ids, local hash embeddings, provenance/freshness, Chroma vector refs, and session/source/semantic edges.
- `packages/tsift-cli/src/lib.rs`: added a fixture-backed regression proving graph-db refresh projects read-only `claude-mem` rows and `tsift semantic` can retrieve them.
- `README.md`, `SPEC.md`, and `packages/tsift-memory/README.md`: documented the read-only adapter boundary.

Verification passed:

- `cargo fmt`
- `tsift rewrite --run 'cargo test -p tsift-cli traversal_projection_materializes_read_only_claude_mem_rows'` (`tart-00d3f4e263`)
- `tsift rewrite --run 'cargo test -p tsift-memory -p tsift-cli'` (`tart-3cf6716314`)
- `tsift rewrite --run 'cargo clippy --workspace --all-targets -- -D warnings'` (`tart-799ab74940`)
- `tsift --envelope __digest-runner --kind log --path . --shell-command 'make check'` (`tart-b728df6109`)
- `tsift rewrite --run 'cargo install --path . --force'` (`tart-70a51bf1d6`)
- installed `tsift memory status . --json` smoke for the expanded memory graph node kinds
- `git diff --check`
- GitHub Actions `main CI` run `26693216013` passed `clippy + test` and `live Convex graph acceptance`

Notes: the `make check` digest still reports the known non-fatal temp-index open signal while the command exits 0. CI annotations were non-blocking: GitHub's Node.js 20 action deprecation notice and the existing live Convex graph no-op annotation for missing `TSIFT_LIVE_CONVEX_GRAPH_URL`. This repo has `CI` and `Release` workflows only; there is no separate tmux-test workflow to inspect. No escalation was required.

Confidence: high.

### Re: do [#tsift-neighborhood-token-budget] — gpt-5

Completed `#tsift-neighborhood-token-budget`.

Implemented in `src/tsift`:
- Added graph-db `output_budget` reports for related, neighborhood, and evidence JSON with selected/candidate counts, estimated token caps, and `dropped_by_budget` diagnostics.
- Added usefulness ranking by semantic score, node/edge kind, hop depth, recency, and source-handle/source-path coverage.
- Enforced per-kind node/edge quotas plus estimated-token caps; neighborhood pages and related retrieval now mark truncation when budget drops occur.
- Evidence packets now apply the budget using shortest-path hop depth overrides, then filter evidence families and paths to retained rows.
- Added regression coverage for neighborhood budget drops, evidence depth ranking, and related/evidence diagnostics.

Commits:
- `src/tsift`: `f726dbd5` (`feat: budget graph-db output`)
- root: `551161b4` (`chore: update tsift submodule`)

Verification:
- `cargo test -p tsift-cli graph_db_output_budget_uses_depth_overrides_for_evidence_rows` passed (`tart-ae5b806717`).
- `cargo test -p tsift-cli` passed (`tart-f73cb4032e`).
- `cargo clippy --workspace --all-targets -- -D warnings` passed (`tart-799ab74940`).
- `make check` passed (`tart-8e3a3b679b`; digest reported existing patch warnings and an expected error-shaped test log line, exit 0).
- `cargo install --path . --force` passed (`tart-70a51bf1d6`; same existing patch warnings).
- `cargo fmt --all -- --check` and `git diff --check` passed.
- GitHub CI `26693680101` passed both `clippy + test` and `live Convex graph acceptance`; annotations were GitHub's Node 20 deprecation and the expected missing live Convex URL note.
- No tmux-test workflow exists for this repo (`gh workflow list` shows `CI` and `Release`).

Escalation: none.
Confidence: high.

### Re: do [#tsift-memory-eval-harness] — gpt-5

Completed `#tsift-memory-eval-harness`.

Implemented in `src/tsift`:
- Added `fixtures/memory-retrieval-eval.json`, a recorded memory retrieval eval fixture derived from the Claude Code Insights report and sampled `claude-mem` failure modes.
- Extended the DCI benchmark fixture/report contract with optional `expected_strategies`, `useful_hits`, `output_tokens`, and `zero_output` metrics.
- Updated strategy summaries and task rows to report useful-hit averages, output-token averages, zero-output failure counts/rates, and per-task most-useful/zero-output lanes.
- Updated `tsift dci-benchmark` human/compact output and SPEC documentation for the new retrieval metrics.
- Added unit and compiled CLI coverage for the memory retrieval fixture.

Changed paths:
- `src/tsift/SPEC.md`
- `src/tsift/fixtures/memory-retrieval-eval.json`
- `src/tsift/packages/tsift-quality/src/dci_benchmark.rs`
- `src/tsift/packages/tsift-cli/src/lib.rs`
- `src/tsift/packages/tsift-cli/tests/exit_code.rs`

Commits:
- `src/tsift`: `49e1387` (`feat: add memory retrieval eval fixture`)
- root: `3c93aa8` (`chore: update tsift submodule`)

Verification:
- `cargo test -p tsift-quality supports_memory_retrieval_metrics` passed (`tart-2fcfad3673`).
- `cargo test -p tsift-cli dci_benchmark_summarizes_memory_retrieval_eval_fixture` passed (`tart-b2bfb8efc0`).
- `cargo run -p tsift-cli -- dci-benchmark --fixture packages/tsift-cli/fixtures/memory-retrieval-eval.json --json` emitted the expected memory strategies and metrics.
- `cargo test -p tsift-quality` passed (`tart-e55fa92b36`).
- `cargo test -p tsift-cli` passed (`tart-6f499b83fd`).
- `cargo clippy --workspace --all-targets -- -D warnings` passed (`tart-799ab74940`).
- `make check` passed after the SPEC edit (`tart-bd08291b25`; digest reported existing patch warnings and an expected error-shaped test log line, exit 0).
- `cargo install --path . --force` passed (`tart-a7c645c6ba`; existing patch warnings only).
- `cargo fmt --all -- --check` and `git diff --check` passed.
- GitHub CI `26694055447` passed both `clippy + test` and `live Convex graph acceptance`; annotations were GitHub's Node 20 deprecation and the expected missing live Convex URL note.
- No tmux-test workflow exists for this repo (`gh workflow list` shows `CI` and `Release`).

Escalation: none.
Confidence: high.
<!-- agent:boundary:358e3f6d -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#tsift-memory-crate-plugin]~
- ~do [#tsift-memory-budget-guard]~
- ~do [#tsift-claude-mem-adapter]~
- ~do [#tsift-neighborhood-token-budget]~
- ~do [#tsift-memory-eval-harness]~
- do [#tsift-summary-extract-gate]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#tsift-summary-extract-gate] Promote missing summary cache from reminder to graph effectiveness gate: context-pack/graph-db related should provide the exact summarize --extract and graph refresh commands required before semantic neighborhoods are trusted.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
- [ ] [#npm-auth-setup] Configure NPM_TOKEN secret + TSIFT_ENABLE_NPM_PUBLISH=true variable in GitHub repo settings
- [ ] [#npm-publish-verify] After auth configured, tag release and verify publish-opencode-plugin CI job succeeds and opencode-tsift appears on npmjs.org
- [ ] [#full-e2e-verify] In fresh project: opencode plugin opencode-tsift && opencode then /tsift-status. On success mark #e2e-install done.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
