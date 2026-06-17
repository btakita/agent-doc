# Plan: tsift Local Model Support for Knowledge Graphs

Created: 2026-06-17

## Context

The current machine has an NVIDIA GeForce RTX 5090 with 32607 MiB available VRAM. At 2026-06-17 15:29 EDT, `nvidia-smi` showed 179 MiB used, with only Xorg listed, so the local KG pipeline can assume roughly 31.8 GiB free before a model is loaded.

Existing tsift surfaces already provide the right integration points:

- `tsift-graph` and `tsift-sqlite` provide the graph substrate.
- `tsift-memgraphrag` projects `tsift-memory` into `GraphProjection` rows and currently uses `tsift-local-hash-v1` as a deterministic semantic placeholder.
- `tsift-surrealdb` exists as a backend candidate.
- `tsift-agent-doc` is the agent-doc/session integration crate.
- `agent-doc plan` already expects graph evidence when an ancestor `.tsift/graph.db` is materialized.

## Model Priority

1. Default extractor/reasoner: Qwen3-32B in a 4-bit GGUF/AWQ-style local serving profile. The model card lists 32K native context and YaRN validation to 131K, which is enough for chunked KG extraction while preserving dense-model quality.
2. Throughput and long-context candidate: Qwen3-30B-A3B-Instruct-2507 quantized. It is a MoE model with 30.5B total / 3.3B activated parameters and 262K native context, useful for many small extraction calls or long consolidated session runs.
3. Embedding/rerank companion: Qwen3-Embedding 0.6B, 4B, or 8B. The 8B card lists 32K context, up to 4096 dimensions, multilingual and code retrieval support; start with 0.6B for low VRAM pressure and benchmark 4B/8B when extraction quality needs denser vectors.
4. Evaluation-only candidate: Qwen3.5-35B-A3B. Keep it behind a benchmark flag until a single-5090 reduced-context profile is proven, because the published full-context SGLang example is multi-GPU.

## Architecture

Add a provider-neutral local model layer and keep graph writes in tsift-native crates:

- `tsift-local-model`: runtime probing, model profiles, provider clients, structured-output request/response types, run leases, and unload/sleep hooks.
- `tsift-kg`: KG extraction contracts shared by `tsift`, `tsift-agent-doc`, and `tsift-memgraphrag`; owns chunking, entity/relation schemas, JSON validation, provenance, and run manifests.
- `tsift-memgraphrag`: replace the hard-coded hash embedding path with a pluggable semantic provider while retaining the hash fallback for deterministic offline tests.
- `tsift-cli`: expose `tsift kg extract`, `tsift kg status`, `tsift kg unload`, and smoke-test commands.
- `tsift-agent-doc`: consume the same `tsift-kg` graph evidence so session planning and queue orchestration do not fork their own KG path.

## Contract Anchors

The normative contract now lives in `src/tsift/specs/local-kg-model.md` and is linked from `src/tsift/SPEC.md`.

- Provider contract: `GpuProbe`, `ModelProfile`, `LocalModelLease`, and `LocalModelProvider`.
- Model ranking: Qwen3-32B default, Qwen3-30B-A3B throughput fallback, Qwen3-Embedding companion profiles, Qwen3.5-35B-A3B benchmark-only.
- Structured extraction: validated JSON entities/relations with stable source-derived ids, confidence bounds, source handles, and kind allowlists.
- Run manifest: `run_id`, model/profile metadata, source and prompt hashes, accepted/rejected counts, and pre/post GPU probes.
- Provenance: KG facts are normal `GraphProjection` rows with `tsift-kg` provenance, content hashes, run manifest references, and source-derived ids that survive repeated runs.

## Run Model

Each KG run should write a manifest containing `run_id`, `model_id`, `model_quant`, `provider`, `source_watermark`, `prompt_hash`, `schema_version`, and `content_hash`. Graph nodes and edges must carry run provenance but stable source-derived ids, so repeated runs can be diffed, superseded, or compared without duplicating the same source fact.

Multiple runs should be serialized by default on a single RTX 5090 when a 30B/32B extractor is loaded. Parallelism is allowed only for CPU stages, small embedding models, or explicitly bounded local-model profiles whose combined estimated VRAM leaves a safety margin.

## VRAM Lifecycle

The local model layer should manage model lifetime explicitly:

- Probe `nvidia-smi` before model load and after unload.
- Use a per-run GPU lease with a single-large-model lock for 30B/32B profiles.
- Prefer process isolation for llama.cpp-compatible runtimes and terminate the worker at run end when unload APIs are not proven.
- For Ollama, use `keep_alive: 0` or `ollama stop` after batch completion.
- For llama.cpp router mode, use `/models/unload` and set `--models-max 1` for the single-5090 default profile.
- Optionally support vLLM sleep mode for future server deployments, but do not make it the first local default.
- Treat VRAM cleanup as a testable acceptance condition: after unload, used VRAM should return near the pre-run baseline plus display-server noise.

## Acceptance Plan

1. Spec the contracts in `src/tsift/SPEC.md` and package READMEs before implementation.
2. Add deterministic unit tests around chunking, schema validation, run manifests, and idempotent graph upserts.
3. Add an integration smoke test using the hash fallback, then a local-runtime ignored test that can run against Qwen on the RTX 5090.
4. Verify two sequential KG runs over the same source tree produce comparable manifests without duplicate graph facts.
5. Verify an interrupted run cleans up model state and does not leave stale graph rows as current.
6. Verify `agent-doc plan` can consume the generated `.tsift/graph.db` evidence through `tsift-agent-doc`.

## Sources

- NVIDIA RTX 5090 specs: https://www.nvidia.com/en-us/geforce/graphics-cards/50-series/rtx-5090/
- Qwen3-32B model card: https://huggingface.co/Qwen/Qwen3-32B
- Qwen3-30B-A3B-Instruct-2507 model card: https://huggingface.co/Qwen/Qwen3-30B-A3B-Instruct-2507
- Qwen3-Embedding-8B model card: https://huggingface.co/Qwen/Qwen3-Embedding-8B
- Qwen3.5-35B-A3B model card: https://huggingface.co/Qwen/Qwen3.5-35B-A3B
- Ollama unload behavior: https://docs.ollama.com/faq
- llama.cpp model management: https://huggingface.co/blog/ggml-org/model-management-in-llamacpp
- vLLM sleep mode: https://docs.vllm.ai/en/latest/features/sleep_mode/
