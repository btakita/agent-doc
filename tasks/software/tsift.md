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

### Re: queue recovery closeout — gpt-5

The recovery preflight committed the `#kgm1` closeout and advanced the active queue to `#kgm2`. No additional repo changes were made in this recovery closeout; `#kgm2` remains the next queued implementation item.

### Re: #kgm2 local model substrate — gpt-5

> **Queue prompt:**
>
> :pushpin: do [#kgm2]


Implemented and committed the local-model substrate for tsift Knowledge Graph work.

Changed paths:
- `src/tsift/packages/tsift-local-model/` adds the provider-neutral local model profile, GPU probe, ranking, status report, and tests.
- `src/tsift/Cargo.toml`, `src/tsift/Cargo.lock`, and `src/tsift/src/lib.rs` wire the new crate into the workspace and public library surface.
- `src/tsift/packages/tsift-cli/Cargo.toml`, `src/tsift/packages/tsift-cli/src/cli.rs`, and `src/tsift/packages/tsift-cli/src/lib.rs` add `tsift local-model status [--json] [--no-probe]`.

Behavior now available:
- `tsift-local-model` exposes RTX 5090-aware `ModelProfile`, `GpuProbe`, `ProfileSelection`, and `LocalModelStatusReport` types.
- The status command probes `nvidia-smi` when available, ranks extractor/embedding profiles, and degrades to the deterministic hash fallback when probing is disabled or unavailable.
- The live probe on this machine reports RTX 5090, driver `610.43.02`, CUDA `13.3`, `32607 MiB` total VRAM, `179 MiB` used, `31971 MiB` free, with `qwen3-32b-q4` recommended for extraction and `qwen3-embedding-0.6b` for embedding.

Commits:
- `src/tsift`: `b0bea57 feat: add local model status substrate`
- parent repo: `0ebc73cb9 tsift: add local model status substrate`

Verification:
- `cargo test -p tsift-local-model`
- `cargo check -p tsift-cli`
- `cargo run -p tsift-cli -- local-model status --json`
- `cargo run -p tsift-cli -- local-model status --no-probe`
- `cargo test -p tsift-cli cli_local_model_status_accepts_json_and_no_probe`
- `git -C src/tsift diff --check`
- `git -C src/tsift diff --cached --check`
- `cargo test --workspace`
- `make check`
- `gh run list --workflow CI --limit 1` was attempted, but this repo has no workflow named `CI`; earlier workflow inspection showed only `Agent Doc`.

Workspace note: unrelated formatting-only dirty files inside `src/tsift` were left unstaged.

Confidence: high. Escalation: force-disk approved for the stuck IPC closeout. The next backlog item is `#kgm3`, replacing the MemGraphRAG semantic path with the shared provider surface.

### Re: #kgm3 MemGraphRAG provider seam — gpt-5

> **Queue prompt:**
>
> :pushpin: do [#kgm3]


Implemented and committed the MemGraphRAG semantic provider seam.

Changed paths:
- `src/tsift/packages/tsift-memgraphrag/src/lib.rs`
- `src/tsift/packages/tsift-memgraphrag/Cargo.toml`
- `src/tsift/Cargo.lock`

Behavior now available:
- Added `SemanticProvider`, `SemanticProviderMetadata`, `SemanticProviderInput`, `SemanticConceptCandidate`, and `SemanticEmbedding` as the pluggable local extraction/embedding seam for MemGraphRAG semantic rows.
- Added `HashSemanticProvider` as the deterministic default, preserving `tsift-local-hash-v1` for tests and no-model fallback behavior.
- Added `append_memory_events_as_traversal_rows_with_provider(...)`; the existing `append_memory_events_as_traversal_rows(...)` now delegates to the hash fallback provider so existing callers keep working.
- Semantic concept nodes now record `semantic_provider`, `semantic_provider_kind`, `semantic_extraction_model`, `semantic_key`, `embedding_provider`, `embedding_model`, `embedding_dimensions`, and `embedding`, and `mentions_concept` edges carry the semantic provider id.
- `tsift-memgraphrag` now depends on `tsift-local-model` for shared `ProviderKind`, keeping the provider seam aligned with the local model substrate.

Commits:
- `src/tsift`: `7fb7f72 feat: add memgraphrag semantic provider seam`
- Parent repo: current HEAD already records `src/tsift` at `7fb7f72` via `4ca28104b`.

Verification:
- `cargo test -p tsift-memgraphrag`
- `cargo check -p tsift-cli`
- `cargo test -p tsift-cli traversal_projection_materializes_cached_semantic_rows`
- `cargo test -p tsift-cli semantic_related_query_uses_persisted_graph_embeddings`
- `cargo test -p tsift-cli graph_db_related_query_uses_semantic_seeds_and_incident_neighborhoods`
- `git -C src/tsift diff --check -- packages/tsift-memgraphrag/Cargo.toml packages/tsift-memgraphrag/src/lib.rs Cargo.lock`
- `git -C src/tsift diff --cached --check`
- `cargo test --workspace`
- `make check`
- `gh run list --workflow CI --limit 1` was attempted, but this repo has no workflow named `CI`.

Workspace note: unrelated formatting-only dirty files inside `src/tsift` remain unstaged.

Confidence: high. Escalation: force-disk approved for the stuck IPC closeout. The next backlog item is `#kgm4`, adding the shared `tsift-kg` extraction pipeline for tsift and agent-doc.

### Re: #kgm4 shared tsift-kg extraction pipeline - gpt-5

> **Queue prompt:**
>
> :pushpin: do [#kgm4]


Implemented the shared `tsift-kg` pipeline for source, session, and memory inputs.

What changed:
- Added `packages/tsift-kg` with document chunking, a `KgExtractor` provider trait for local-model calls, deterministic `HashKgExtractor` fallback, JSON entity/relation validation, `GraphProjection` materialization, SQLite upsert support, and backend candidate metadata.
- Wired `tsift-kg` into the tsift workspace and root `tsift::kg` export.
- Re-exported the same crate through `tsift-agent-doc::kg` so agent-doc can use the shared library instead of duplicating extraction logic.
- Added tests for mixed input chunking, relation endpoint validation, projection materialization, and SQLite upserts.

Commits:
- `b5c3132` in `src/tsift`: `feat: add shared kg extraction pipeline`
- `92756bc16` in the parent repo: `tsift: add shared kg extraction pipeline`

Verification:
- `cargo test -p tsift-kg`
- `cargo check -p tsift`
- `cargo check -p tsift-agent-doc`
- `cargo test --workspace`
- `git -C src/tsift diff --check -- Cargo.toml Cargo.lock src/lib.rs packages/tsift-agent-doc/Cargo.toml packages/tsift-agent-doc/src/lib.rs packages/tsift-kg`
- `git -C src/tsift diff --cached --check`
- `make check`
- `gh run list --workflow CI --limit 1` still reports `could not find any workflows named CI`, matching the earlier repo state.

Workspace note: the pre-existing unrelated formatting-only dirty files under `src/tsift` remain unstaged. The next queued item is `#kgm5` for lifecycle and VRAM cleanup controls.

### Re: #kgm5 lifecycle and VRAM cleanup controls - gpt-5

> **Queue prompt:**
>
> :pushpin: do [#kgm5]


Implemented provider-neutral local-model lifecycle and cleanup controls.

What changed:
- Added timestamped `GpuProbe` records, `LocalModelLease`, unload action descriptions, lifecycle reports, cleanup statuses, and before/after VRAM cleanup evaluation in `tsift-local-model`.
- Added provider-specific cleanup action planning for llama.cpp router unload, Ollama `keep_alive: 0`/`ollama stop`, vLLM sleep mode, process-exit fallback, and no-GPU fallback profiles.
- Added cleanup proof logic that passes when post-unload VRAM returns within tolerance, accepts increases accounted for by non-tsift GPU processes, and fails when a tsift/llama/Ollama/vLLM process remains or VRAM stays materially above baseline.
- Added `tsift local-model unload` with live or synthetic probes, JSON/human output, configurable tolerance, provider endpoint/pid metadata, and `--strict` failure mode for run gates.
- Documented the lifecycle contracts in `packages/tsift-local-model/README.md`.

Commits:
- `d26043e` in `src/tsift`: `feat: add local model lifecycle cleanup`
- `a06f91f0d` in the parent repo: `tsift: add local model lifecycle cleanup`

Verification:
- `cargo test -p tsift-local-model`
- `cargo test -p tsift-cli cli_local_model_unload_accepts_probe_and_strict_flags`
- `cargo check -p tsift-cli`
- `cargo run -p tsift-cli -- local-model unload --pre-used-mib 200 --post-used-mib 800 --provider-pid 42 --json`
- `cargo run -p tsift-cli -- local-model unload --pre-used-mib 200 --post-used-mib 2000 --strict --json` exited non-zero as expected.
- `git -C src/tsift diff --check -- packages/tsift-local-model packages/tsift-cli/src/cli.rs packages/tsift-cli/src/lib.rs`
- `git -C src/tsift diff --cached --check`
- `cargo test --workspace`
- `make check`
- `gh run list --workflow CI --limit 1` still reports `could not find any workflows named CI`.

Workspace note: the unrelated pre-existing dirty files under `src/tsift` remain unstaged. The next queued item is `#kgm6` for multi-run verification.

### Re: #kgm6 multi-run verification - gpt-5

> **Queue prompt:**
>
> :pushpin: do [#kgm6]


Completed the multi-run verification queue item for the local KG model work.

Implemented in `src/tsift`:

- Added `KgMultiRunVerificationReport` and `verify_projection_multi_run_stability` in `tsift-kg`.
- Added a two-run KG extraction regression that verifies stable node/edge ids across repeated identical runs, then upserts both projections into one SQLite graph and confirms semantic entities/relations are not duplicated.
- Added an interrupted-run cleanup regression in `tsift-local-model` proving an orphaned provider process blocks VRAM cleanup proof and produces a process-exit fallback action.
- Documented the multi-run stability helper in the `tsift-kg` README.

Commits:

- `a012eeb` in `src/tsift`: `test: add kg multi-run verification`
- `6d68cf5b6` in the parent repo: `tsift: add kg multi-run verification`

Verification run:

- `cargo test -p tsift-kg`
- `cargo test -p tsift-local-model`
- `cargo run -p tsift-cli -- graph stable_id . --json --no-tagpath --limit 1`
- `cargo run -p tsift-cli -- memory status . --json`
- `git -C src/tsift diff --check -- packages/tsift-kg packages/tsift-local-model/src/lib.rs`
- `git -C src/tsift diff --cached --check`
- `cargo test --workspace`
- `make check`

The CI lookup was attempted with `gh run list --workflow CI --limit 1`, but this repository still reports `could not find any workflows named CI`; local verification passed. The pre-existing unrelated dirty files under `src/tsift` remain unstaged.

### Re: local model status — gpt-5

> **Queue prompt:**
>
> What local model is setup to work with tsift? Is it being used now? 


The configured local model path is the new `tsift-local-model` profile set, not a live always-on service. On this RTX 5090, `tsift local-model status --json` recommends:

- Extractor: `qwen3-32b-q4` (`Qwen/Qwen3-32B-GGUF`, llama.cpp, 4-bit) as the default quality KG extractor/reasoner for a clear 5090.
- Embedding/rerank companion: `qwen3-embedding-0.6b` (`Qwen/Qwen3-Embedding-0.6B-GGUF`) as the default low-pressure embedding profile.
- Fallback extractor profile: `qwen3-30b-a3b-instruct-2507-q4` for throughput/long-context extraction.
- Benchmark-only candidate: `qwen3.5-35b-a3b-q4`, currently not selectable on the live probe because its estimated footprint exceeds available free VRAM.
- Built-in fallback: `tsift-local-hash-v1`, used for deterministic tests/offline runs and the current hash fallback KG/semantic paths.

It is not being used as a live local model right now. The live probe reported the RTX 5090 available with 32,607 MiB total VRAM, 31,971 MiB free, and no GPU model processes. `pgrep -af "ollama|llama|vllm|qwen|ggml"` also found no matching provider process.

Current implementation status: the local-model substrate, KG extraction trait, lifecycle/VRAM cleanup checks, and multi-run verification are in place. The active default code paths still have deterministic hash fallback providers (`HashKgExtractor` / `HashSemanticProvider`) unless a caller supplies a real local extractor/provider. So the answer is: Qwen3-32B Q4 is the intended/default local KG extractor profile for this machine, but it is not currently loaded or serving tsift work.

### Re: Please set up the local model to start working. What happens if I use corky gpu while the model is running? — gpt-5

> **Queue prompt:**
>
> :pushpin: Please set up the local model to start working. What happens if I use corky gpu while the model is running?


I installed the current local `tsift` CLI from `src/tsift`, so `/home/brian/.cargo/bin/tsift` now has the `local-model` commands that were implemented in the repo. `tsift --version` reports `0.1.70`, and `tsift local-model status` now works from the normal shell path.

Live local-model state:
- GPU: NVIDIA GeForce RTX 5090, 32,607 MiB total VRAM, 179 MiB used, 31,971 MiB free.
- Recommended extractor: `qwen3-32b-q4`, estimated at 28,692 MiB, selectable on the current free VRAM.
- Recommended embedding companion: `qwen3-embedding-0.6b`, estimated at 2,736 MiB, selectable.
- Cleanup proof: `tsift local-model unload --json` reports cleanup `Proven` at the current baseline, with post-unload VRAM still 179 MiB.

What is not running yet: no actual model server is installed or active. `ollama` and `llama-server` are not on PATH, and there are no matching GPU model provider processes. So the setup is now ready at the `tsift` CLI/profile/probe layer, but tsift still falls back to deterministic hash providers unless a real llama.cpp/Ollama/vLLM provider is installed, started, and wired into a caller.

For `corky gpu`: the installed `corky 0.29.0` does not expose a `gpu` subcommand, and this installed build does not expose `transcribe` either. The local `src/corky` source tree describes GPU transcription as `corky transcribe` built with `transcribe-cuda`, not as a `corky gpu` command.

If a GPU-enabled Corky transcription run uses the same local RTX 5090 while the Qwen3 32B tsift extractor is loaded, the two jobs contend for VRAM and CUDA. The tsift extractor is an exclusive large-GPU profile; after its estimated 28,692 MiB footprint, only about 3.3 GiB of currently-free VRAM remains. A Corky CUDA Whisper/diarization job could OOM, fail to allocate CUDA buffers, force one workload to slow down hard, or make tsift's post-run VRAM cleanup proof fail. It should not corrupt repo data; the risk is GPU memory pressure, latency, and failed model/transcription runs.

Practical rule: unload or stop the tsift model provider before GPU Corky work, then re-run `tsift local-model status` after Corky finishes. If concurrency is necessary, use the hash fallback or a small embedding profile instead of the `qwen3-32b-q4` extractor.

Commands/proof:
- `cargo install --path . --force` from `src/tsift`
- `tsift local-model status --json`
- `tsift local-model status`
- `tsift local-model unload --json`
- `nvidia-smi` / `nvidia-smi --query-gpu=...`
- `which ollama` and `which llama-server` both missing
- `corky --version`, `corky --help`, `corky gpu --help`, and `corky help transcribe`

Changed paths: no repo source paths changed by me; the installed binary at `/home/brian/.cargo/bin/tsift` was replaced. Push: none, because there was no repo code commit to push. Escalation: force-disk was used after the editor IPC path repeatedly failed to prove delivery. Confidence: high for the installed tsift setup and GPU contention assessment; medium for Corky command naming because the installed binary and local source agree on GPU transcription being feature-gated, but neither exposes a literal `corky gpu` subcommand here. The linked `headroom` repo is a local-first context compression/proxy/MCP project, not a GPU model runtime: https://github.com/chopratejas/headroom

### Re: GPU control pane and mid-run model swap — glm-5.2

> **Queue prompt:**
>
> Should we have a control pane that manages the GPU resources. Reducing the model? Can the model be reduced in the middle of an agent-doc run or quickly switchout out between tsift calls?

**Short answer: not a control pane — a cooperative lease registry. And yes to per-call profile pinning + a `swap` action.**

**On the control pane.** The substrate already declares the right vocabulary (`LeaseMode::Exclusive/Shared/CpuOrHash`, `ConcurrencyClass`, `LocalModelLease`) but the lease is currently descriptive only — nothing acquires, holds, or enforces it. So the gap is enforcement, not modeling. For this machine (1 RTX 5090, ~31.9 GiB free, a handful of polite consumers: tsift, corky, headroom, agent-doc), a full daemon control pane is over-engineered. Daemon cost (IPC contract, lifecycle, crash recovery, preempt semantics) outweighs benefit at this scale. Right-size answer: a **cooperative GPU lease registry** — one state file (`/run/tsift/gpu-lease.json` or `~/.tsift/gpu-lease.json`) keyed by `profile_id` with `{pid, acquired_at, lease_mode, vram_baseline_mib}`; producers check before probe, stale leases (pid gone or older than TTL) are reclaimable. Cheaper than a daemon, recovers cleanly on crash, fits the single-GPU reality. Reserve the real control pane (preempt + multi-GPU scheduling) for when there is proven contention — more than 1 GPU or more than 3 simultaneous consumers. Tracked as `#gctrl1` (do) and `#gctrlIce` (icebox).

**On mid-run reduction.** Two distinct capabilities, both feasible:

- **Profile downgrade inside an agent-doc run** — yes. Today the 32B extractor sits at ~28.7 GiB, leaving only ~3.3 GiB; if Corky or another job needs the GPU mid-run, the agent-doc cycle should be able to drop to the 0.6B embedding profile or to the hash fallback for low-stakes phases (digest/verify/diff), then reload later. The clean shape is `#gctrl2`: a `--profile <id>` flag on tsift commands that touch the model, so the caller pins/downgrades per call without mutating global state.

- **Quick swap between tsift calls** — yes for small profiles, no for the 32B. Realistic load cost on the 5090: ~2 s for `qwen3-embedding-0.6b`, ~10–30 s for `qwen3-32b-q4`. So "quickly switch out" is genuinely quick at the small end and visibly slow at the big end. `#gctrl3` adds a `tsift local-model swap --to <profile>` action (unload + existing VRAM-cleanup proof + load) so a downgrade is one command instead of two, with the cleanup gate enforced between.

**Why not "reducing the model" via quantization tier steps**: that is also possible (treat `-q4`/`-q3`/`-q2` as separate profiles), but per-call profile pinning already covers the common case — you switch profile, you don't re-quantize mid-flight. Quantization tiers would just be more entries in `default_model_profiles()`; congestion-aware `status` can already rank them via `rank_profiles_for_role`. No new substrate needed.

**Recommended path this cycle**: green-light `#gctrl1` (lease registry, the foundational piece), then `#gctrl2` (per-call `--profile`) and `#gctrl3` (`swap` action). Defer `#gctrlIce` (daemon + preempt + multi-GPU) until contention is proven. No code changed this turn — this is a design decision seeking your green-light before implementation.

### Re: #gctrl1 cooperative GPU lease registry — glm-5.2

> **Queue prompt:**
>
> do [#gctrl1]


> **Queue prompt:**
>
> :pushpin: do [#gctrl1]


Implemented, installed, and committed the cooperative GPU lease registry for tsift-local-model.

What changed:
- `packages/tsift-local-model/src/lib.rs` adds the registry types and the pure + file-backed APIs: `GpuLeaseRecord`, `GpuLeaseRegistry`, `GpuLeaseAcquisition{,Status}`, `GpuLeaseConflict`, `GpuLeaseRelease{,Outcome}`; `resolve_lease_file`, `is_pid_alive` (via `kill -0`), `read_lease_registry`, `write_lease_registry` (atomic temp + fsync + rename), `prune_stale_leases`, `apply_acquire`, `apply_release`, `acquire_lease`, `release_lease`, `show_registry`, `format_lease_show_human`. Also exposed `current_unix_seconds` as pub and cleaned one pre-existing collapsible-`if` so the clippy gate stays green.
- `packages/tsift-local-model/Cargo.toml` adds `serde_json = "1"`.
- `packages/tsift-cli/src/cli.rs` adds `LocalModelCommand::Lease { Acquire, Release, Show }` plus a `LeaseCommand` enum and `json_output()` accessor.
- `packages/tsift-cli/src/lib.rs` wires the new subcommand via `cmd_local_model_lease`, including `--strict` exit-nonzero on conflict and JSON/human output.
- `packages/tsift-local-model/README.md` documents the file location, schema, concurrency rules, staleness contract, and CLI surface.

Concurrency rules match `LeaseMode`: `Exclusive` = one live holder per profile; `Shared` = many; `CpuOrHash` = bypass (no registry entry). A holder is stale when `kill -0` fails or when `idle_ttl_seconds > 0` and the age exceeds the TTL; stale holders are pruned on every acquire/release/show and reclaimed by a new acquirer. Pure logic takes an `is_alive: impl Fn(u32) -> bool` closure so tests inject deterministic liveness; file wrappers use the real `is_pid_alive`.

Behavior now available:
- `tsift local-model lease show [--lease-file F] [--include-stale] [--json]`
- `tsift local-model lease acquire --profile <id> [--holder-pid P] [--holder-command C] [--idle-ttl-seconds S] [--vram-baseline-mib M|--no-probe] [--lease-file F] [--strict] [--json]`
- `tsift local-model lease release --profile <id> [--holder-pid P] [--lease-file F] [--json]`
- `acquire --strict` exits non-zero on conflict (verified: pid 99999 challenger against a live shell-pid holder returned exit 1 with `Error: gpu lease for "qwen3-32b-q4" is held by pid=<live>`).

Resolution order for the registry path: `--lease-file` → `$TSIFT_LEASE_FILE` → `$XDG_STATE_HOME/tsift/gpu-lease.json` → `~/.tsift/gpu-lease.json` → `./.tsift/gpu-lease.json`.

Commits:
- `src/tsift`: `a5e717d feat: add cooperative GPU lease registry (#gctrl1)`
- parent repo: `e3534df1e tsift: add cooperative GPU lease registry (#gctrl1)`

Verification:
- `cargo test -p tsift-local-model` → 22 passed (10 new lease tests + 12 existing).
- `cargo test -p tsift-cli --lib cli_local_model_lease` → 3 passed (acquire/release/show parsing).
- `cargo clippy -p tsift-local-model -p tsift-cli --all-targets -- -D warnings` → clean.
- `cargo test --workspace` → all green (largest suite 378 passed).
- `make check` → exit 0.
- `cargo install --path . --force` → installed `tsift 0.1.70`.
- Smoke: empty `show` → acquire by real shell pid → conflict attempt by dead pid 99999 returns `Conflict` with holder info and `--strict` exits 1 → human `show` displays the live lease → `release` returns `Released` with `remaining_holders: 0`.
- `git -C src/tsift diff --cached --check` and parent `git diff --cached --check` → clean.
- `gh run list --workflow CI --limit 1` could not run because the root repo has no workflow named `CI`; `gh workflow list` shows only `Agent Doc`.

Workspace note: the pre-existing unrelated dirty files under `src/tsift` (`session_cost.rs`, `session_review.rs`, `infra.rs`, `exit_code.rs`, `log_digest.rs`) remain unstaged as before.

Confidence: high. `#gctrl1` done. Next queued: `#gctrl2` (`--profile` pin on tsift KG/semantic commands).
<!-- agent:boundary:21a361d7 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- ~~do [#gctrl1]~~
- do [#gctrl2]
- do [#gctrl3]
- do [#gctrlice]
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#gctrl2] [recommended] Add `--profile <id>` flag to tsift KG/semantic commands so an agent-doc cycle can pin or downgrade the model per-call. When omitted, keep current behavior (rank by free VRAM). Allows a long agent-doc run to pick a small/hash profile during low-stakes phases without changing global state.
- [ ] [#gctrl3] [recommended] Add `tsift local-model swap --to <profile>` lifecycle action distinct from `unload`: unload current profile, run the existing VRAM-cleanup proof, then load the target profile and emit a combined LocalModelLifecycleReport. Use it to implement mid-run downgrades (qwen3-32b-q4 -> qwen3-embedding-0.6b or hash fallback) without the caller orchestrating two commands.
- [ ] [#gctrlice] [recommended] Icebox: revisit a full GPU control-pane daemon (with preempt and multi-GPU scheduling) only when proven need — more than 1 GPU, or more than 3 concurrent GPU consumers running at the same time. Until then the cooperative lease registry (#gctrl1) is the right-size answer.
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
