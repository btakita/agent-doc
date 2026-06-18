---
agent_doc_session: tsift-v0.1
agent: opencode
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
queue: stop
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260617-232347.md`*

Compacted content:
- Archived 14 response topic(s): queue recovery closeout; #kgm2 local model substrate; #kgm3 MemGraphRAG provider seam; 11 more
- Prior summary/context: prior compacted content: Archived 6 response topic(s): #advance-review; small model vs MemGraphRAG; model support mat...

### Re: queue drain — Port 8080 free; fmt closeout — glm-5.2

> **Queue prompt:**
>
> Port 8080 is free now


Drained the `#spec-test-build-install-commit-push` queue head (`Port 8080 is free now`). The 8080 signal unblocked the local-model/WordPress conflict path documented in `packages/tsift-local-model/README.md` (#portconf default `http://127.0.0.1:8080/models/unload`).

**Outcome — full cycle complete:**
- **Spec:** no spec change needed; the working-tree delta was pure rustfmt drift (no semantic edits).
- **Tests:** `make check` green — `cargo clippy --workspace --all-targets -- -D warnings` clean, full `cargo test --workspace` passing, opencode-plugin-test 8/8 + `pack:check` ok.
- **Build + install:** `cargo install --path . --locked` → `tsift 0.1.70` replaced in `~/.cargo/bin`.
- **Commit + push:** completed the outstanding `cargo fmt` drift across 8 files (session_cost, session_review, cli, infra, lib, exit_code test, log_digest, local-model lib) and pushed `cd648b5..0e41e4b` to origin/main (`style: apply cargo fmt across agent-doc, cli, digest, local-model`).

Queue head consumed. No new actionable backlog items — the remaining backlog head `#vmax` (crates.io OIDC trusted-publishing registration) is still operator-gated.

### Re: local-model runtime + service-vs-lazy — glm-5.2

> **Queue prompt:**
>
> Is the tsift local model running now? If not, let's start it. Should it be a service or should we make this a lazy instance?

**1. Not running — and nothing is installed to run it.** No provider listens on 8080 (llama.cpp) / 11434 (ollama) / 8000 (vllm); the RTX 5090 is idle (179 / 32607 MiB, no compute procs); the GPU lease registry is empty. `tsift local-model status` reports `qwen3-32b-q4` selectable (28692 MiB est) and `qwen3-embedding-0.6b` selectable, but there are **no model files** on disk and **no provider binary** (`ollama`, `llama-server`/`llama.cpp`, `vllm` are all absent). So there is nothing to start yet.

**2. tsift won't "start" it itself** — `tsift local-model` ships only `status | unload | lease | resolve | swap` by design (provider-neutral, no daemon). Starting means launching an external provider.

**3. Service vs lazy → lazy, by spec.** [`specs/local-kg-model.md`](../../src/tsift/specs/local-kg-model.md) (Runtime Provider Contract + VRAM Cleanup Acceptance) and the `tsift-local-model` crate (#gctrl1 lease registry / #gctrl2 per-call preference / #gctrl3 swap) are built around **acquire → use → unload-with-cleanup-proof**: cooperative GPU leases with idle TTL, single-GPU serialization, and a mandatory pre/post VRAM probe. The spec's non-goals explicitly list "parallel 30B/32B model serving." There is intentionally **no tsift-owned daemon** — the provider (llama.cpp / ollama / vllm) is treated as an on-demand resource, and `build_unload_actions` sends `keep_alive:0` / router-unload / vLLM-sleep / process-exit to release it. A lazy instance matches both the code and the spec; a persistent service would fight the unload/cleanup contract.

**To start it (lazy path) — needs your call on provider + model:**
- **Easiest:** install `ollama`, `ollama serve`, `ollama run qwen3-32b-q4` (~29 GB pull), then `export TSIFT_OLLAMA_ENDPOINT=...` (ollama keeps the model warm until tsift sends `keep_alive:0`).
- **Lighter smoke first:** `ollama run qwen3-embedding-0.6b` (~3 GB) to exercise extract/embed without the 29 GB pull.
- **No-download path:** `tsift local-model resolve --profile hash` uses `tsift-local-hash-v1` (0 MiB, always selectable) to validate the lease/resolve plumbing end-to-end on the GPU-free fallback.

Captured as backlog `#lmlazy` [recommended]. Want me to proceed with ollama + the embedding-0.6b smoke (then the 32B extractor), or just wire the hash fallback for now?
<!-- agent:boundary:e63ccc4c -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#vmax] [operator-verify] Enable crates.io OIDC publishing for tsift (advances review #1td4; code already shipped in fe4144b/f29f7a2, release.yml crates job uses rust-lang/crates-io-auth-action@v1). Operator-gated steps (need crates.io account + browser 2FA — agent cannot perform): (1) On crates.io register a Trusted Publisher for EACH of the 23 tsift crates — GitHub repo btakita/tsift, workflow release.yml, no environment: tsift-core, tsift -md-ast, tsift-graph, tsift-sqlite, tsift-algorithms, tsift-resolution, tsift-cache, tsift-tokensave, tsift-libsql, tsift-index, tsift-summarize, tsift-quality, tsift-agent-doc, tsift-digest, tsift-search, tsift-status, tsift-session, tsift-memory, tsift-memgraphrag, tsift-surrealdb, tsift-cli, tsift, tsift-sim-world. (2) Enable the gate var: gh variable set TSIFT_ENABLE_CRATES_PUBLISH --body true. (3) Force-move the v0.1.70 tag to HEAD and push to re-trigger; confirm the publish-crates.io job authenticates via OIDC (no CARGO_REGISTRY_TOKEN) and publishes.
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
