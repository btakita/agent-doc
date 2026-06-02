# Plan: Integrate lazily-rs for Caching and Dependency Graph

**Status:** Planning
**Created:** 2026-05-31
**Session:** `tasks/agent-doc/agent-doc-bugs.md`

## Goal

Wire `lazily-rs` into agent-doc to cache filesystem-derived computations (`project_root_for_doc`, config loading, frontmatter parsing, SSH resolution) behind a reactive dependency graph. Two Context lifetimes: short-lived CLI run and long-lived Actor/watch daemon.

## Current State

- `project_root_for_doc(file) -> Option<PathBuf>` — walks up from file looking for `.agent-doc/`, called from ~10 sites (project_config_io.rs, frontmatter_io.rs, session_check.rs, session_accretion.rs, lint_gate.rs)
- `load_project_for_doc(file) -> ProjectConfig` — calls `project_root_for_doc` then reads/parses `.agent-doc/config.toml`, called from ~8 sites
- `resolve_ssh_context_inputs(file)` — calls `canonicalize` + `load_project_for_doc` + `project_root_for_doc`, called from frontmatter_io.rs
- `parse_for_file(content, file) -> Frontmatter` — calls `resolve_ssh_context_inputs`, then `parse_with_ssh_resolver`
- Multiple duplicate `find_project_root` implementations across watch.rs, compact.rs, notify.rs, ffi.rs, archive_index.rs, supervisor/cwd.rs
- No lazily-rs dependency in any agent-doc Cargo.toml

## Architecture

### Context Lifetimes

> **Multithreading update (2026-06-02):** lazily-rs now ships a `ThreadSafeContext`
> (mutex-backed, `Send + Sync + 'static`) alongside the single-threaded `Context`.
> `SlotHandle<T>` / `CellHandle<T>` / `EffectHandle` are `Send + Sync` when `T` is.
> This changes the lifetime mapping below: **RunContext** stays the lightweight
> single-threaded `Context` (one CLI invocation runs on one thread), but
> **ActorContext** must use `ThreadSafeContext` because the supervisor/watch
> daemon genuinely shares one reactive graph across threads (idle-queue-watch
> thread, IPC listener thread, event loop). `loom`/`tokio` features exist for the
> thread-safe model and async integration. This supersedes the old "lazily-rs is
> single-threaded (RefCell)" risk below.

| Context | lazily-rs backing | Lifetime | Owner | Invalidation |
|---------|-------------------|----------|-------|-------------|
| **RunContext** | `Context` (single-threaded) | Single CLI invocation (`preflight → write → commit`) | Created at top of `main.rs` dispatch, dropped at exit | Implicit — process exit clears everything |
| **ActorContext** | `ThreadSafeContext` (`Send + Sync`, mutex-backed) | Long-lived (watch daemon, supervisor, session actor) | Created at daemon/actor startup, held in `SupervisorShared` or `watch.rs` event loop, shared across worker threads | Explicit — `slot.clear()` or `cell.set()` on file/config change events |

### Slot/Cell Graph

```
Cell: file_path (PathBuf)                    — input, set per invocation
 │
 ├─ Slot: canonical_path(file_path)          — std::fs::canonicalize
 │   │
 │   ├─ Slot: project_root(canonical_path)   — walk up to .agent-doc/
 │   │   │
 │   │   ├─ Slot: config_path(project_root)  — .agent-doc/config.toml
 │   │   │   │
 │   │   │   └─ Slot: project_config(config_path) — parse_project_toml
 │   │   │       │
 │   │   │       ├─ Slot: ssh_context(project_config, canonical_path) — resolve SSH inputs
 │   │   │       │
 │   │   │       └─ Slot: frontmatter(content, ssh_context) — parse with SSH resolver
 │   │   │
 │   │   └─ Slot: snapshot_path(project_root) — .agent-doc/snapshots/<hash>.md
 │   │
 │   └─ Slot: doc_relative(canonical_path, project_root) — relative path within project
```

### Phase Data Flow

Each phase accesses the graph through the same Context:

```
preflight  →  project_root, project_config, snapshot_path
plan       →  frontmatter, project_config
run        →  frontmatter, project_config, ssh_context
write      →  project_root, frontmatter, project_config
commit     →  project_root, frontmatter
```

Within a CLI run, all slots compute at most once. Within an Actor, slots persist and invalidate on external events.

## Phased Implementation

### Phase 1: Foundation (#lr-found-1)

**Add dependency + define graph types.**

- Add `lazily-rs` dependency to `agent-doc-orchestration/Cargo.toml`
- Create `agent-doc-orchestration/src/graph.rs`:
  - Define `RunContext` newtype wrapper around `lazily::Context`
  - Define slot/cell handles: `FilePathCell`, `CanonicalPathSlot`, `ProjectRootSlot`, `ProjectConfigSlot`, `SshContextSlot`, `FrontmatterSlot`
  - Register all compute closures
- Write unit tests for the graph in isolation

**Verification:** `cargo test -p agent-doc-orchestration` passes, graph compiles, slot computations produce correct results.

### Phase 2: Deduplicate project_root_for_doc (#lr-dedup-2)

**Replace all `find_project_root` / `project_root_for_doc` calls with graph slot access.**

- Consolidate the 8+ `find_project_root` implementations into one function used by the graph slot
- Update callers in: project_config_io.rs, frontmatter_io.rs, session_check.rs, session_accretion.rs, lint_gate.rs, watch.rs, compact.rs, notify.rs, ffi.rs, archive_index.rs, supervisor/cwd.rs
- For CLI paths: create `RunContext` at dispatch, thread `&RunContext` to callers
- For Actor paths: access through `ActorContext` held in supervisor/watch state

**Verification:** `cargo test` full suite passes, no behavior change, all duplicate implementations removed.

### Phase 3: ProjectConfig and SSH resolution slots (#lr-config-3)

**Move `load_project_for_doc` and `resolve_ssh_context_inputs` into graph slots.**

- `ProjectConfigSlot` depends on `ProjectRootSlot` — reads and parses `.agent-doc/config.toml`
- `SshContextSlot` depends on `ProjectConfigSlot` + `CanonicalPathSlot`
- Update `frontmatter_io.rs` to use graph slots instead of direct calls
- Update `session_check.rs`, `lint_gate.rs` to use graph slots

**Verification:** `cargo test` passes, SSH resolution still works for SSH-targeted documents.

### Phase 4: Frontmatter slot (#lr-fmt-4)

**Move `parse_for_file` into a graph slot.**

- `FrontmatterSlot` depends on `SshContextSlot` + file content (passed as parameter or separate cell)
- Update plan.rs, run.rs, write.rs to use graph slot
- Frontmatter parsing remains in `agent-doc-core` (pure) — the slot wraps it with I/O

**Verification:** `cargo test` passes, frontmatter parsing produces identical results.

### Phase 5: ActorContext for long-lived processes (#lr-actor-5)

**Wire `ActorContext` (backed by `ThreadSafeContext`) into watch daemon and supervisor.**

- `ActorContext` wraps `ThreadSafeContext` (not `Context`) so the same graph can be shared across the supervisor's worker threads (idle-queue-watch, IPC listener) — store it as `Arc<ActorContext>` in `SupervisorShared`.
- `ActorContext` held in `watch.rs` event loop state alongside existing `FileState`/`StreamState` maps
- `ActorContext` held in `SupervisorShared` for session actor
- On file change events: `CellHandle::set(&ctx, &mut file_path, new_path)` or `SlotHandle::clear(&ctx, project_root)` to invalidate stale cached data (the `ThreadSafeContext` graph lock serializes concurrent reads/invalidations)
- On config file change: clear `ProjectConfigSlot` and all dependents transitively

**Verification:** Watch daemon and supervisor tests pass. Manual test: edit `.agent-doc/config.toml` during a running watch loop, verify config changes take effect without restart.

### Phase 6: Content / Frontmatter / Component slots (#lr-content-6)

**Add content-derived slots that eliminate 20-60 redundant reads/parses/scans per cycle.**

- `DocContentCell` (Cell<String>) — raw document content, set after `read_to_string`
- `FrontmatterSlot` (Slot<Arc<Frontmatter>>) — depends on `DocContentCell` + `SshContextSlot`; wraps `parse_for_file`
- `ComponentsSlot` (Slot<Arc<Vec<Component>>>) — depends on `DocContentCell`; wraps `component::parse`
- Wire into session_check guard functions so each reads from slot instead of re-reading/parsing file
- Invalidation: `DocContentCell` set on file read; downstream slots auto-clear

**Verification:** `cargo test -p agent-doc-orchestration` — new slot tests pass; existing tests unchanged.

### Phase 7: Snapshot paths / cycle state / snapshot content slots (#lr-cycle-7)

**Consolidate path and state lookups that recompute canonicalize + project_root + doc_hash 20-40x per cycle.**

- `DocHashSlot` (Slot<String>) — depends on `CanonicalPathSlot`; SHA-256 of canonical path
- `SnapshotPathsSlot` — depends on `ProjectRootSlot` + `DocHashSlot`; derives `path_for`, `lock_path_for`, `pending_path_for`, `baseline_path_for`
- `CycleStateSlot` (Slot<Arc<CycleState>>) — depends on `SnapshotPathsSlot`; loads cycle state JSON once
- `SnapshotContentSlot` (Slot<Arc<String>>) — depends on `SnapshotPathsSlot`; loads snapshot with flock once
- Thread `RunContext` into `snapshot.rs`, `cycle_state.rs` callers
- Invalidation: `SnapshotContentSlot.clear()` after save/delete; `CycleStateSlot.clear()` after save

**Verification:** `cargo test -p agent-doc-orchestration` — new slot tests pass; existing tests unchanged.

### Phase 8: Head content / snapshot commit status slots (#lr-head-8)

**Eliminate 3-8 git subprocess spawns per cycle by caching `git show HEAD` output.**

- `HeadContentSlot` (Slot<Arc<Option<String>>>) — depends on `CanonicalPathSlot` + `ProjectRootSlot`; spawns `git show HEAD:<relative>` once
- `SnapshotCommitStatusSlot` (Slot<bool>) — depends on `SnapshotContentSlot` + `HeadContentSlot`; compares snapshot vs HEAD once
- Thread into preflight, session_check, closeout callers
- Invalidation: `HeadContentSlot.clear()` after `git::commit`

**Verification:** `cargo test -p agent-doc-orchestration` — new slot tests pass; existing tests unchanged.

### Phase 9: Wire RunContext into ops_log / template_io / patch / harness detection (#lr-wire-9)

**Thread RunContext into remaining modules that independently walk project root or resolve config.**

- `ops_log::log_op` accepts `Option<&RunContext>` to skip root walk
- `template_io::load_component_configs` / `load_max_lines_configs` use `rc.project_config()` instead of `load_project_from_doc`
- `patch.rs` uses `rc.project_root()` instead of manual walk
- `detect_harness()` result cached as `HarnessSlot` (compute-once, env vars are process-static)
- `notify.rs` uses `rc.project_root()` instead of local `find_project_root`

**Verification:** `cargo test` full suite passes; no behavior change.

### Phase 10: Global config / session registry slots for ActorContext (#lr-actor-10)

**Add process-level and project-level cached slots for long-lived actors.**

- `GlobalConfigSlot` (Slot<Arc<Config>>) — reads `~/.config/agent-doc/config.toml` once per CLI run; in ActorContext, invalidates on file-watcher event
- `SessionRegistrySlot` (Slot<Arc<SessionRegistry>>) — depends on `ProjectRootSlot`; loads `sessions.json` once; invalidates on file-watcher event
- Wire into route.rs, sync.rs, resync.rs, gc.rs callers that currently call `sessions::load()` multiple times

**Verification:** `cargo test` full suite passes; no behavior change.

## Risks and Mitigations

| Risk | Mitigation |
|------|-----------|
| ~~lazily-rs is single-threaded (RefCell)~~ **(resolved 2026-06-02)** | lazily-rs now ships `ThreadSafeContext` (`Send + Sync`, mutex-backed). RunContext uses the single-threaded `Context` (one-thread CLI run); ActorContext uses `ThreadSafeContext` because the supervisor/watch daemon shares one graph across worker threads. |
| `ThreadSafeContext` graph-lock contention in the supervisor | Keep slot graphs coarse and reads short; the lock is held only for graph traversal, not for the underlying fs/parse work. Benchmark with the `instrumentation` feature (lock timing) if contention appears; verify the model with the `loom` feature. |
| API surface change (threading `&Context` everywhere) | Phase gradually; each phase only changes one layer of callers. RunContext and ActorContext expose the same slot/cell read API so call sites are backing-agnostic. |
| Performance regression from graph overhead | Benchmark pre/post Phase 2; slot overhead is HashMap lookup + borrow, negligible vs filesystem stat |
| lazily-rs not yet on crates.io | Use path dependency: `lazily-rs = { path = "../../lazily-rs" }` |

## Open Questions

- Should `CellHandle::set(&ctx, value)` be added to lazily-rs first? (Current API is `Context::set_cell()`)
- Should the graph be per-file (one context per tracked document) or per-process (one context with file-keyed cells)?
- How to handle config hot-reload in long-lived ActorContext — file watcher on `.agent-doc/config.toml` or clear on next cycle?
