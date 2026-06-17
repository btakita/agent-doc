---
agent_doc_session: tsift-v0.1
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#bugs-observed': Please add any bugs from observed behavior to the backlog.,
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260617-192612.md`*

Compacted content:
- Archived 6 response topic(s): #advance-review; small model vs MemGraphRAG; model support matrix; 3 more
- Prior summary/context: prior compacted content: **[#1td4]** crates.io publish job migrated to OIDC trusted publishing. **Code complete and o...
- `gh run list --workflow CI --limit 1` could not run because the root repo has no workflow named `CI`; `gh workflow list` shows only `Agent Doc`.
- `make check` passed.
- `git diff --check -- tasks/software/plan-tsift-local-kg-model-support.md` passed.
4. Evaluation-only: Qwen3.5-35B-A3B, gated until a reduced-context single-5090 profile is proven.
3. Embedding companion: Qwen3-Embedding 0.6B/4B/8B, starting small and benchmarking up when vector quality matters.
2. Throughput/long-context candidate: Qwen3-30B-A3B-Instruct-2507 quantized, because it has 30.5B total / 3.3B activated parameters and 262K native context.
1. Default extractor/reasoner: Qwen3-32B quantized local profile, because the RTX 5090 has enough free VRAM for a 4-bit 32B-class model and Qwen3-32B has 32K native context with YaRN validation beyond that.
- `gh run list --workflow CI --limit 1` could not run because the root repo has no workflow named `CI`; `gh workflow list` shows only `Agent Doc`.
- `make check` passed after the contract docs were updated.
- `git diff --check -- tasks/software/plan-tsift-local-kg-model-support.md` passed.
- `git -C src/tsift diff --check -- SPEC.md specs/local-kg-model.md packages/tsift-memgraphrag/README.md packages/tsift-agent-doc/README.md packages/tsift-graph/README.md` passed.
- Earlier planning commit remains `30eeef705`: `docs: plan tsift local kg models`
- `17e46aa13` in the parent repo: `docs: update tsift local kg plan`
- `475dbf4` in `src/tsift`: `docs: specify local kg model contracts`
- `tasks/software/plan-tsift-local-kg-model-support.md` now points to the normative contract and summarizes the contract anchors.
- `src/tsift/packages/tsift-graph/README.md` now states that local KG facts augment, not replace, deterministic tree-sitter graph extraction.
- `src/tsift/packages/tsift-agent-doc/README.md` now states that agent-doc consumes `.tsift/graph.db`/`tsift-*` evidence and must not own a separate local-model lifecycle.
- `src/tsift/packages/tsift-memgraphrag/README.md` now anchors MemGraphRAG to the shared local KG contract while retaining `tsift-local-hash-v1` as the deterministic fallback.
- `src/tsift/SPEC.md` now links the new local KG spec from the stable spec map.
- `src/tsift/specs/local-kg-model.md` defines the provider-neutral local KG contract: `GpuProbe`, `ModelProfile`, `LocalModelLease`, `LocalModelProvider`, RTX 5090 model ranking, structured entity/relation schema, run manifests, graph provenance, multi-run semantics, and VRAM cleanup acceptance.
<!-- agent:boundary:e988f29f -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- do [#kgm1]
- do [#kgm2]
- do [#kgm3]
- do [#kgm4]
- do [#kgm5]
- do [#kgm6]
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#kgm2] [queue] Implement the tsift local-model substrate from tasks/software/plan-tsift-local-kg-model-support.md: GPU/VRAM probe, RTX 5090 profile, Qwen3-32B default ranking, Qwen3-30B-A3B throughput fallback, Qwen3-Embedding companion profiles, and CLI status output.
- [ ] [#kgm3] [queue] Replace the tsift-memgraphrag hash-only semantic path from tasks/software/plan-tsift-local-kg-model-support.md with a pluggable local embedding/extraction provider while retaining tsift-local-hash-v1 as deterministic fallback for tests.
- [ ] [#kgm4] [queue] Add the shared tsift-kg extraction pipeline from tasks/software/plan-tsift-local-kg-model-support.md for tsift and agent-doc: chunk source/session/memory inputs, call the local model, validate JSON entities/relations, and upsert GraphProjection rows into SQLite plus backend candidates.
- [ ] [#kgm5] [queue] Add model lifecycle and VRAM cleanup controls from tasks/software/plan-tsift-local-kg-model-support.md: per-run lease, idle TTL, Ollama keep_alive=0, llama.cpp router unload, process-kill fallback, nvidia-smi before/after proof, and single-5090 concurrency guard.
- [ ] [#kgm6] [queue] Add multi-run verification from tasks/software/plan-tsift-local-kg-model-support.md: two sequential KG runs, interrupted-run cleanup, no duplicate/stale graph facts, no orphaned model processes, VRAM returns near baseline, make check, and tsift graph/memory smoke tests.
- [ ] [#vmax] [operator-verify] Enable crates.io OIDC publishing for tsift (advances review #1td4; code already shipped in fe4144b/f29f7a2, release.yml crates job uses rust-lang/crates-io-auth-action@v1). Operator-gated steps (need crates.io account + browser 2FA — agent cannot perform): (1) On crates.io register a Trusted Publisher for EACH of the 23 tsift crates — GitHub repo btakita/tsift, workflow release.yml, no environment: tsift-core, tsift-md-ast, tsift-graph, tsift-sqlite, tsift-algorithms, tsift-resolution, tsift-cache, tsift-tokensave, tsift-libsql, tsift-index, tsift-summarize, tsift-quality, tsift-agent-doc, tsift-digest, tsift-search, tsift-status, tsift-session, tsift-memory, tsift-memgraphrag, tsift-surrealdb, tsift-cli, tsift, tsift-sim-world. (2) Enable the gate var: gh variable set TSIFT_ENABLE_CRATES_PUBLISH --body true. (3) Force-move the v0.1.70 tag to HEAD and push to re-trigger; confirm the publish-crates.io job authenticates via OIDC (no CARGO_REGISTRY_TOKEN) and publishes.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#1td4] Release infra: crates.io publish job in .github/workflows/release.yml is MIGRATED to OIDC trusted publishing (commit fe4144b, on origin/main f29f7a2; uses rust-lang/crates-io-auth-action@v1, id-token: write, no CARGO_REGISTRY_TOKEN). Code complete. Remaining is operator-gated and tracked as actionable backlog #vmax: register the Trusted Publisher per crate on crates.io (23 crates), set repo var TSIFT_ENABLE_CRATES_PUBLISH=true, then re-tag v0.1.70 to verify OIDC publish. Stays gated until operator completes crates.io registration.
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
