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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260601-204619.md`*

Compacted content:
- Archived 2 response topic(s): lazily-rs cache contracts and SurrealDB; SurrealDB parity next steps
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: #surreal-backend-eval — gpt-5

> **Queue prompt:**
>
> [#surreal-backend-eval]


Implemented and pushed `surrealdb` as a `graph-db backend-eval` candidate while keeping it held behind the experimental promotion gate.

- Added `GraphDbExperimentalBackend::Surrealdb`, parser aliases (`surrealdb`, `surreal`, `surreal-db`), default candidate inclusion, CLI help, graph-db schema text, adapter-spike listing, and native-adapter-required promotion checks.
- Kept the implementation on the existing provider-neutral read-only row snapshot path so `--candidate surrealdb` loads the same rows and compares signatures against SQLite.
- Extended backend-eval conformance coverage to assert SurrealDB read-only metadata, operation success, SQLite parity, metrics, adapter-spike presence, and hold-gate checks.
- Updated `README.md` and `SPEC.md` so SurrealDB appears in backend-eval candidate/spec surfaces.

Verification passed:

- `cargo test -p tsift-cli cli_parses_graph_db_backend_eval_surrealdb_candidate`
- `cargo test -p tsift-cli --test graph_db_conformance graph_db_backend_eval_benchmarks_candidate_stores_against_sqlite`
- `make check`
- `cargo build`
- `cargo install --path . --force`

`cargo build` and `cargo install` only emitted the existing unused local patch warnings for `agent-kit`, `tagpath`, and `tmux-router`.

CI check: latest GitHub Actions CI run `26769887559` was already red on `main`, but both jobs failed before test steps ran. The annotations on both check runs report GitHub runner `No space left on device` writing runner diagnostics, so there was no CI test/clippy failure to reproduce locally. This repo has only `CI` and `Release` workflows and no `make tmux-ci` target.

Commits pushed:

- `btakita/tsift@074e7b4` (`feat(graph): add surrealdb backend-eval candidate`)
- `btakita/agent-loop@a3576dc3b` (`chore: update tsift submodule`)
<!-- agent:boundary:98031817 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~[#surreal-backend-eval]~
- [#surreal-parity-suite]
- [#surreal-native-adapter]
- [#surreal-promotion-gate]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
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
