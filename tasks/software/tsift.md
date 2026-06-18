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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260618-021302.md`*

Compacted content:
- Archived 2 response topic(s): queue drain; local-model runtime + service-vs-lazy
- Prior summary/context: prior compacted content: Archived 14 response topic(s): queue recovery closeout; #kgm2 local model substrate; #kgm3 M...

### Re: #lmlazy — ollama-served KG extraction shipped — glm-5.2

> **Queue prompt:**
>
> [#lmlazy]


Drained the `[#lmlazy]` queue head. The lazy Ollama provider is now wired end-to-end for real model-backed KG extraction — the gap the smoke surfaced (no `tsift kg` command, no inference client, hash-only extractor) is closed and committed (`8b9e71b`, pushed `0e41e4b..8b9e71b`).

**What landed (commit `8b9e71b`):**
- **`tsift-kg::OllamaKgExtractor`** (`packages/tsift-kg/src/ollama.rs`) — ureq `/api/chat` client with a structured-output JSON schema, plus `repair_extraction_payload` that drops relations referencing unknown entity ids and clamps confidence to 0..=1. The first live 32B run surfaced exactly this (a hallucinated `e8` relation); the strict shared validator stays strict, the model path repairs before it.
- **Ollama-native profiles** in `tsift-local-model` (`qwen3-32b-q4-ollama`, `qwen3-embedding-0.6b-ollama`) — `ProviderKind::Ollama` + `OllamaKeepAliveZero`, `model_ref` matching the `hf.co/…` pull tags. The unload lifecycle now POSTs `127.0.0.1:11434/api/generate` (`keep_alive:0`) instead of llama.cpp's 8080.
- **`tsift kg extract`** CLI (`tsift-cli`) — stdin/`--input` → resolve profile/model → `extract_documents_to_projection` → optional `--graph-db` upsert.

**Live verification (RTX 5090):**
- `tsift kg extract --profile qwen3-32b-q4-ollama` → 9 entities / 12 relations from a sample (tsift-local-model, lease registry, OllamaKgExtractor, …).
- Unload action resolves to ollama `keep_alive:0`; VRAM 29000 → 179 MiB after `ollama stop`.
- `make check` green (clippy `-D warnings` + full suite + opencode-plugin-test); 15 new `tsift-kg` tests (prompt body, schema, JSON-cleaning, repair, pipeline round-trip, + a `TSIFT_OLLAMA_SMOKE`-gated live test).

Marked `#lmlazy` `[x]` done. Follow-ups (not blocking): execute the unload action (today's run used `ollama stop` directly — `build_unload_actions` plans but does not yet POST), and an ollama embedding profile wired into `tsift semantic`.

### Re: active local-model KG usage — glm-5.2

> **Queue prompt:**
>
> :pushpin: Does sift now actively use local models with the kg?


> **Queue prompt:** Does sift now actively use local models with the kg?

**Partially — one path is live, the spec-promised substrates are not.**

| Surface | Active? | Evidence |
|---|---|---|
| `tsift kg extract` CLI | **Yes** | `packages/tsift-cli/src/commands/kg.rs:39-44` instantiates `OllamaKgExtractor`, runs `extract_documents_to_projection`, and optionally upserts into `.tsift/graph.db`. Live-verified on RTX 5090 last cycle (commit `8b9e71b`). |
| `tsift-memgraphrag` | **No** | No `tsift-kg` Cargo dep (`packages/tsift-memgraphrag/Cargo.toml`); the crate only imports `tsift_local_model::ProviderKind` as a type (`src/lib.rs:6`). Spec line 27-28 (`memgraphrag consumes tsift-kg providers`) is **unfulfilled**. |
| `tsift-agent-doc` | **No** | Declares `tsift-kg` as a Cargo dep but the only use is `pub use tsift_kg as kg;` in `src/lib.rs:7` — re-export, no call site. Spec line 29-30 (`agent-doc consumes graph evidence`) is **unfulfilled**. |
| `tsift kg` subcommands | Partial | `KgCommand` enum (`packages/tsift-cli/src/cli.rs:1094`) has only `Extract`. Spec line 31-32 calls for `status`, `unload`, and smoke-test commands — **missing**. |
| VRAM unload | **No** | Per last cycle: `build_unload_actions` plans but does not POST the ollama `keep_alive:0`; operator ran `ollama stop` directly. |

**Bottom line:** local models are wired into the KG only through the standalone `tsift kg extract` command. The downstream consumers the spec names (`tsift-memgraphrag`, `tsift-agent-doc`) do not yet drive local-model inference, and the operational surface (`status`/`unload`/smoke) is incomplete.

Did **not** apply the `#spec-test-build-install-commit-push` preset this cycle: the prompt was a state-assessment question, not a change request — no spec/test/code changes were made, so build/install/commit/push does not apply. Captured the four concrete gaps above as `#kgmrs`, `#kgcliext`, `#kgunloadpost`, `#kgaduse` in `agent:backlog`; `#kgmrs` is the top item (it is the largest spec-vs-reality delta).
<!-- agent:boundary:a6345b8c -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- do [#kgmrs]
- do [#kgcliext]
- do [#kgunloadpost]
- do [#kgaduse]
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#kgmrs] Wire tsift-kg into tsift-memgraphrag so it actually invokes OllamaKgExtractor (today memgraphrag has no tsift-kg Cargo dep and only references tsift_local_model::ProviderKind as a type — local-kg-model.md spec line 27-28 is unfulfilled). Plan: packages/tsift-memgraphrag/src/lib.rs
- [ ] [#kgcliext] Add tsift kg status / tsift kg unload / tsift kg smoke subcommands per local-kg-model.md spec line 31-32 (today KgCommand only has Extract). Plan: packages/tsift-cli/src/cli.rs + packages/tsift-cli/src/commands/kg.rs
- [ ] [#kgunloadpost] Make tsift-local-model build_unload_actions actually POST the ollama keep_alive:0 (today it plans but does not POST — operator ran ollama stop manually). Plan: packages/tsift-local-model/src/lib.rs
- [ ] [#kgaduse] Activate or drop tsift-agent-doc's tsift-kg Cargo dep (today only 'pub use tsift_kg as kg;' re-export, no call site — dead dep per local-kg-model.md spec line 29-30). Plan: packages/tsift-agent-doc/src/lib.rs
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
