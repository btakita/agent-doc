# agent-doc-orchestration — compile baseline

Tracks the orchestration-crate extraction (`#adoc-orchestration-crate` / `#bz6s`).
See `tasks/agent-doc/plan-agent-doc-orchestration-extraction.md` for the wave plan.

The headline cold-build comparison (CLI shell vs full crate vs core) becomes
meaningful only after Wave 5 (prune CLI deps), once the heavy orchestration
dependency tree (tokio/hyper/rustls/interprocess/notify/rusqlite/git/zstd/
tmux-router) has actually left the `agent-doc` CLI crate.

## Wave log

- **Wave 0 + 1a** — scaffold the crate (depends on `agent-doc-core`) and move
  the one dependency-free leaf module, `ipc_socket` (414 LOC, deps:
  `anyhow`/`interprocess`/`serde_json`). Main re-exports via
  `pub use agent_doc_orchestration::ipc_socket`. 11 inline tests moved with it.
  All FFI symbols (39) still export from `libagent_doc.so`.

## Pending waves

- **1b** — `sessions` + `supervisor/` require their non-cluster closure first
  (`input_diag`, `env`, `harness`, `project_controller`, `session_actor`,
  `startup_miss`, `test_support`). Moving them naively would create a circular
  `agent-doc` ↔ `agent-doc-orchestration` dependency. Resolve that closure
  before the move.
- **2–5** — state/capture, git, orchestrators, prune (per the plan).
