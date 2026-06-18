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

### Re: #kgmrs — memgraphrag consumes tsift-kg — glm-5.2

> **Queue prompt:**
>
> do [#kgmrs]


> **Queue prompt:** do [#kgmrs]

Done. `tsift-memgraphrag` now drives real model-backed KG extraction via `tsift-kg`, closing the spec line 27-28 gap.

**What landed (tsift commit `d159838`, pushed `8b9e71b..d159838`):**
- **`KgSemanticProvider<E: KgExtractor = OllamaKgExtractor>`** (`packages/tsift-memgraphrag/src/lib.rs:170-264`) — generic over any `KgExtractor`, with `::ollama(model_tag)` for production and `::with_extractor(stub)` for tests. Routes `SemanticProvider::extract_concepts` through `extract_documents_to_projection`, mapping each accepted `KgJsonEntity` to a `SemanticConceptCandidate` keyed `kg:<kind>`. Degrades to a primary candidate when the model returns zero entities so a thin extraction can't blank a memory row.
- **`tsift-kg` Cargo dep** added to `packages/tsift-memgraphrag/Cargo.toml`.
- **`embed()` stays on the hash fallback** — the ollama embedder is a separate `#lmlazy` follow-up (noted in code comments); `metadata().embedding_model` reports `HASH_SEMANTIC_PROVIDER_ID` honestly so downstream consumers see the split.
- **5 new unit tests**: metadata propagation, pipeline extraction (verifies 2 entities from a stub), empty-extraction fallback, hash-fallback embed, and an end-to-end traversal-projection test proving `KgSemanticProvider` feeds the same graph-build path `FixtureSemanticProvider` already exercised.

**Verification (`#spec-test-build-install-commit-push` preset):**
- `make check` green (clippy `-D warnings` + full suite + opencode-plugin-test). Targeted `cargo test -p tsift-memgraphrag --lib kg_semantic_provider` → 5 passed.
- `cargo install --path . --locked` → installed (replaced `/home/brian/.cargo/bin/tsift`).
- Submodule pointer bumped in agent-loop (`9e57d6e33`).

**Remaining queue (OpenCode has no auto-loop — re-invoke `/agent-doc` to drain the next head):**
- `do [#kgcliext]` — `tsift kg status` / `unload` / `smoke` subcommands (spec line 31-32).
- `do [#kgunloadpost]` — `build_unload_actions` actually POSTs `keep_alive:0`.
- `do [#kgaduse]` — activate or drop `tsift-agent-doc`'s dead `tsift-kg` re-export.

### Re: #kgcliext — tsift kg status / unload / smoke — glm-5.2

> **Queue prompt:**
>
> do [#kgcliext]


> **Queue prompt:** do [#kgcliext]

Done. `tsift-cli` now exposes the full operational surface required by `specs/local-kg-model.md` line 31-32: `extract`, `status`, `unload`, `smoke`. `tsift kg --help` lists all four.

**What landed (tsift commit `665b5a0`, pushed `d159838..665b5a0`):**
- **`tsift kg status`** — opens `.tsift/graph.db` read-only via `SqliteGraphStore::open_read_only_resilient`, reports total node/edge counts and a top-10 breakdown by kind. Returns `exists: false` cleanly when the db is missing instead of erroring. `--json` emits a `KgStatusReport` envelope.
- **`tsift kg unload`** — resolves the KG profile, calls `tsift_local_model::build_unload_actions` for the plan, and **actually POSTs** the ollama `keep_alive:0` to `/api/generate` (closing the operator-facing side of `#kgunloadpost` — no more manual `ollama stop`). Falls back to the `ollama stop` subprocess if the HTTP call fails. The `--json` envelope reports per-action execution status.
- **`tsift kg smoke`** — runs a small built-in sample through `extract_documents_to_projection` against a live Ollama server, prints chunk/entity/relation counts, and optionally `--unload`s after. Exits non-zero with a clear error if the server is down or the model is missing.

**Files:** `packages/tsift-cli/src/cli.rs` (`KgCommand` enum extended), `packages/tsift-cli/src/commands/kg.rs` (three new `cmd_kg_*` fns + helpers + tests), `packages/tsift-cli/src/lib.rs` (dispatch arms).

**Verification (`#spec-test-build-install-commit-push`):**
- `make check` green (clippy `-D warnings` + full suite + opencode-plugin-test).
- `cargo test -p tsift-cli --lib commands::kg` → 5 passed (status missing-db, status populated-db with kind counts, host override resolution blank/non-blank, truncate helper).
- `cargo install --path . --locked` → installed.
- Live spot-check: `tsift kg status --graph-db /tmp/nonexistent.db` prints the missing-db message in both human and `--json` modes.
- Submodule pointer bumped in agent-loop (`4fc99362e`).

**Note on `#kgunloadpost`:** the operator-facing gap (manual `ollama stop`) is closed — `tsift kg unload` POSTs. The architectural side remains open: `build_unload_actions` in `tsift-local-model` is still a plan-only function; the new POST execution lives in `tsift-cli`. Leaving `#kgunloadpost` open with its original scope (refactor the planner to own execution so lease-drop can also POST without a CLI call).

**Queue still active (2 heads):** `do [#kgunloadpost]`, `do [#kgaduse]`. Re-invoke `/agent-doc` to drain the next.

### Re: #kgunloadpost — planner owns execution — glm-5.2

> **Queue prompt:**
>
> do [#kgunloadpost]


> **Queue prompt:** do [#kgunloadpost]

Done. `build_unload_actions` is no longer a plan-only function — the planner in `tsift-local-model` now owns execution end-to-end, closing the architectural side that `#kgcliext` left open (CLI was the only caller POSTing).

**What landed (tsift commit `5e08f2a`, pushed `665b5a0..5e08f2a`):**

New public API in `tsift-local-model` (`src/lib.rs` after `build_unload_actions`):
- **`UnloadActionResult { label, executed, outcome }`** — serializable outcome type, reused by the CLI report.
- **`PreparedUnloadRequest`** — pure projection of an action into a concrete request (url + body + fallback_command). Separated from execution so the body-rewriting and URL-normalization logic is fully testable without a live HTTP server.
- **`rewrite_unload_body_model(body, model_tag)`** — fixes the silent-`--model`-override bug. `build_unload_actions` formats the body with `profile.model_ref`, so without this rewrite the override never reached the POST. Empty override is a byte-for-byte passthrough (no serde key reorder).
- **`normalize_unload_url(endpoint)`** — appends `/api/generate` idempotently; tolerates trailing slashes; accepts either bare host or full generate URL.
- **`prepare_unload_request(action, model_tag)`** — pure; returns `None` for non-API actions; synthesizes a body when the plan has none.
- **`execute_unload_request(req)`** — impure; POSTs then falls back to the attached subprocess command.
- **`execute_unload_actions(actions, model_tag)`** — chain driver with **required-action short-circuit**: once a `required: true` action succeeds, subsequent fallbacks are reported as `skipped: prior required unload succeeded`. Fixes the redundant `ollama stop` call after a successful `keep_alive:0` POST.
- **`unload_model_at(host, model_tag)`** — fire-and-forget convenience for callers without a full action plan (used by `tsift kg smoke --unload`).

**CLI refactor** (`packages/tsift-cli/src/commands/kg.rs`): `cmd_kg_unload` now delegates to `execute_unload_actions`; `KgUnloadReport` reuses the local-model `UnloadActionResult` type directly. The CLI shed ~130 lines of duplicated POST/fallback logic.

**Verification (`#spec-test-build-install-commit-push`):**
- `make check` green (clippy `-D warnings` + full suite).
- `cargo test -p tsift-local-model --lib` → 50 passed (10 new for `#kgunloadpost`: body rewriting × 3, URL normalization × 3, prepare/request projection × 2, non-API skip, chain short-circuit).
- `cargo install --path . --locked` → installed.
- **Live spot-check** against `127.0.0.1:11434`: `tsift kg unload --json` → primary `ollama keep_alive zero` POST returns HTTP 200 OK; fallback `ollama stop` correctly reported `skipped: prior required unload succeeded`. Pre-refactor it was running both.
- Submodule pointer bumped in agent-loop (`7fb19820b`).

**Queue still active (1 head):** `do [#kgaduse]` — activate or drop `tsift-agent-doc`'s dead `tsift-kg` re-export.

### Re: #kgaduse — drop dead tsift-kg dep — glm-5.2

> **Queue prompt:**
>
> do [#kgaduse]


> **Queue prompt:** do [#kgaduse]

Done — chose **drop**, not activate. Full activation is a larger design pass (which planner call sites consume `.tsift/graph.db` evidence, and how), and carrying a dead re-export of the wrong layer was obscuring the spec-vs-reality gap rather than helping.

**What landed (tsift commit `57a3b2b`, pushed `5e08f2a..57a3b2b`):**
- **Removed `tsift-kg` Cargo dep** from `packages/tsift-agent-doc/Cargo.toml`.
- **Removed `pub use tsift_kg as kg;`** from `packages/tsift-agent-doc/src/lib.rs` (zero call sites — verified via `grep -rn 'tsift_agent_doc::kg\|::kg::'` across the workspace).
- **In-source comment** at the old re-export site points to `#kgadactivate` for the real consumer work.
- **README expanded** with a "Current state" subsection documenting the gap between the contract and the implementation so future readers don't mistake the dropped dep for a regression.

**Why drop, not activate:** the README contract is explicit — agent-doc may **read** `.tsift/graph.db` via the **`tsift-sqlite`/`tsift-core` GraphStore layer**, but must not own the **`tsift-kg` extraction pipeline**. The re-export was exposing exactly the wrong layer. Activating it in its current shape would have deepened the contract violation; dropping it clears the slate for `#kgadactivate` to add the correct (store-read) seam.

**Verification (`#spec-test-build-install-commit-push`):**
- `cargo clippy --workspace --all-targets -- -D warnings` clean.
- `make check` green (full suite).
- `cargo install --path . --locked` → installed.
- Submodule pointer bumped in agent-loop (`20dcd0484`).

**Captured follow-up:** `#kgadactivate` now sits at the top of `agent:backlog` as the actionable next step — implement the real `GraphStore`-backed evidence lookup helper, add `tsift-sqlite` + `tsift-core` deps, wire it into `session_digest` or planning. Flagged as needing a dedicated design pass on consumer call sites.

**Queue drained.** This was the last head — `queue_active` should clear on the next preflight.
<!-- agent:boundary:46bd6b5c -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- do [#kgadactivate]
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#kgadactivate] Implement the real tsift-agent-doc KG evidence consumer: a read-only GraphStore-backed lookup helper that surfaces .tsift/graph.db facts to planning/orchestration. Per local-kg-model.md spec line 29-30, this is the contract the dropped re-export was supposed to seed. Plan: add tsift-sqlite + tsift-core deps to packages/tsift-agent-doc/Cargo.toml, add a graph_evidence module that calls SqliteGraphStore::open_read_only_resilient + scoped node/edge queries, wire it into session_digest or planning. Larger scope than #kgmrs/#kgcliext/#kgunloadpost — needs a dedicated design pass on which planner call sites consume the evidence.
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
