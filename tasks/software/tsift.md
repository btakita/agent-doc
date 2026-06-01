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
queue_active: false
---

## Status

<!-- agent:status patch=replace -->


<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260601-162307.md`*

Compacted content:
- Archived 1 response topic(s): lazily-rs cache invalidation
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ do [#lazily-cache-contracts]. #spec-test-build-install-commit-push
❯ Would SurrealDB be a better fit than Sqlite? See https://surrealdb.com/blog/surrealdb-3-x-by-the-numbers
### Re: lazily-rs cache contracts and SurrealDB — gpt-5

Done. I added a dedicated `### lazily-rs Cache Contracts` section to `src/tsift/SPEC.md` that records each lazily-rs-backed cache key, lifetime, invalidation trigger, and regression proof for `SummaryCache`, `InspectScopeGuard`, `StatusCheckCache`, `ResolveEdgesCache`, `GraphStore::neighborhood`, and `GraphStore::ranked_neighborhood`.

I also added `spec_documents_lazily_rs_cache_contracts` in `packages/tsift-cli/tests/exit_code.rs`. It verifies the packaged CLI spec stays synced with the workspace spec and asserts the cache-contract table keeps the required key/invalidation/test coverage strings.

On SurrealDB vs SQLite: not a default replacement for tsift yet. The May 29, 2026 SurrealDB 3.x post reports durable, same-hardware benchmark runs and claims strong embedded-mode write/scan results versus SQLite, which is interesting evidence for a future adapter spike: https://surrealdb.com/blog/surrealdb-3-x-by-the-numbers. But tsift's current promotion bar is broader than CRUD throughput: provider-neutral `GraphStore` parity, projection load/write behavior, install portability, and multi-process/read-only lock behavior across real, full-projection, high-degree, and deep-chain workloads. SQLite remains the safer default until a SurrealDB adapter clears that existing backend-eval gate.

Changed paths:
- `src/tsift/SPEC.md`
- `src/tsift/packages/tsift-cli/tests/exit_code.rs`
- superproject gitlink `src/tsift`

Verification:
- Passed: `tsift rewrite --run 'cargo test -p tsift-cli spec_documents_lazily_rs_cache_contracts'`
- Passed: `tsift --envelope __digest-runner --kind test --path . --shell-command 'make check'`
- Passed: `tsift rewrite --run 'cargo build --workspace'`
- Passed: `tsift rewrite --run 'cargo install --path . --locked --force'`
- Passed: `tsift --version` reports `tsift 0.1.62`
- Format caveat: workspace `cargo fmt --check` and file-level `rustfmt --check packages/tsift-cli/tests/exit_code.rs` are red on pre-existing formatting drift outside this change; I did not apply a broad formatting rewrite.

CI / tmux evidence:
- Latest pushed tsift CI run: https://github.com/btakita/tsift/actions/runs/26769887559
- The initial run and one failed-job retry both failed from GitHub runner infrastructure before project commands completed, with `System.IO.IOException: No space left on device` while the Actions runner wrote diagnostic logs. I treated this as external CI-start/runner failure, not a code failure.
- I found no `tmux`/`tmux-ci` target or tmux-specific job in this repo's configured CI workflow.
- The superproject has no workflow named `CI`.

Commits / push:
- Pushed `btakita/tsift@88838e8` (`docs: document lazily cache contracts`)
- Pushed `btakita/agent-loop@bbb2b65a8` (`chore: update tsift submodule`)

Escalation: none.
Confidence: high for the local implementation and verification; GitHub CI is blocked by external runner disk exhaustion rather than a project test signal.
❯ What are #next-steps to have surrealdb parity with sqlite?

### Re: SurrealDB parity next steps — gpt-5

Top backlog item: `#surreal-backend-eval`.

I added the next steps in priority order. The right first move is not to make SurrealDB a normal `GraphDbBackend` yet; it should enter through `graph-db backend-eval` as an experimental candidate, because that is already the SQLite promotion gate. The current spec requires candidate backends to match SQLite signatures across real, full-projection, high-degree, and deep-chain workloads before promotion, and the code already routes candidate prototypes through `GraphDbExperimentalBackend` plus `graph_db_backend_eval_report_for_store`.

Priority order:

1. `#surreal-backend-eval` — add a SurrealDB backend-eval candidate that loads provider-neutral rows and reports SQLite parity signatures.
2. `#surreal-parity-suite` — extend conformance coverage so SurrealDB is checked on every GraphStore-backed operation that SQLite currently owns.
3. `#surreal-native-adapter` — only after read-only parity is locked, spike a feature-gated optional SurrealDB adapter that writes provider-neutral projection rows to an embedded/file-backed store.
4. `#surreal-promotion-gate` — run repeated real/full/synthetic backend-eval samples, including install portability and multi-process/read-only behavior, before considering promotion.

SurrealDB is plausible enough to test now: the official Rust docs show embedded Rust usage with in-memory and file-backed RocksDB modes, and the 3.x benchmark post reports strong embedded-mode numbers against SQLite. I would still treat those as a reason to run tsift's own backend-eval gate, not as enough evidence to replace SQLite. The tsift bar is narrower and stricter: GraphStore parity, projection writes/load, path/evidence/conflict/dispatch behavior, default install cost, and lock semantics on our actual workloads.

Sources checked:
- SurrealDB Rust embedding docs: https://surrealdb.com/docs/languages/rust/embedding
- SurrealDB 3.x benchmark post: https://surrealdb.com/blog/surrealdb-3-x-by-the-numbers

Changed paths: session document/backlog only.
Commands: `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, `tsift --envelope search`, targeted `rg`/`nl` inspections, official SurrealDB doc lookup.
Verification: source tests not run because this was a backlog/planning response with no code changes. `agent-doc finalize` will run the document write/commit/session-check closeout.
Escalation: none.
Confidence: high for the ordering; medium for SurrealDB production suitability until the new backend-eval evidence exists.
<!-- agent:boundary:a36cfb54 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#surreal-backend-eval] Add SurrealDB as a graph-db backend-eval candidate before exposing it as a normal GraphDbBackend: extend GraphDbExperimentalBackend parsing/defaults/help/spec so 'tsift graph-db --json backend-eval --candidate surrealdb' loads provider-neutral rows and reports SQLite parity signatures.
- [ ] [#surreal-parity-suite] Extend graph_db_conformance backend-eval coverage for SurrealDB candidate parity across status/counts, edge lookup, edge property scans, incident scans, neighborhood, related retrieval, path tiers, evidence, conflict-matrix, and dispatch-trace.
- [ ] [#surreal-native-adapter] Spike a feature-gated optional SurrealDB adapter crate that writes provider-neutral projection rows to an embedded/file-backed SurrealDB store without adding SurrealDB to the default cargo build/install path.
- [ ] [#surreal-promotion-gate] Run repeated real, full-projection, synthetic high-degree, and synthetic deep-chain backend-eval samples for the SurrealDB adapter, including install portability and multi-process/read-only lock behavior, then update the promotion gate only if it preserves SQLite parity and beats SQLite on every required workload.
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
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
