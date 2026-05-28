# Plan: `agent-doc-orchestration` crate extraction (`#ysv9` / `#adoc-orchestration-crate`)

Scoping document produced by `do [#ysv9]`. This is the **next big extraction** after `#adcr` (which produced `agent-doc-core`, published as v0.1.0). It splits the orchestration layer out of the main crate, leaving a thin CLI shell and completing a clean 3-layer architecture:

```
agent-doc (CLI)  →  agent-doc-orchestration  →  agent-doc-core
  entrypoint +        route/git/sessions/         pure data layer
  command dispatch    ipc/supervisor/sync/...     (published 0.1.0)
```

## Motivation

- `agent-doc-core` already cold-builds in **9.87s / 74 crates**; the main crate is **129s / 266 crates**. The gap is almost entirely the orchestration dependency tree: `tokio`, `hyper`/`reqwest`/`rustls`/`ureq`, `interprocess`, `notify`, `rusqlite` (bundled SQLite), `git2`-style git, `zstd`, `tmux-router`.
- The orchestration cluster is **~44,500 LOC (~29% of the 154K main crate)**. Extracting it lets the CLI shell + command dispatch stay near core's build profile, and gives a clean dependency-injected boundary for testing.
- Acceptance criterion #1 of `#adcr` is satisfied; this extraction is the follow-on the baseline justified.

## Module inventory (the cluster)

| Module | LOC | Role |
|--------|-----|------|
| `sync.rs` | 11,365 | tmux 2D layout reconciler |
| `git.rs` | 8,430 | commit/branch/squash + missed-patchback self-heal |
| `route.rs` | 8,069 | harness trigger → pane routing |
| `start.rs` | 5,571 | start harness in tmux pane |
| `supervisor/` | 3,534 | PTY/process supervision (8 files) |
| `repair.rs` | 3,310 | orphaned-response / stale-cycle recovery |
| `sessions.rs` | 1,738 | session registry (sessions.json) |
| `capture.rs` | 1,070 | durable response-capture ledger |
| `cycle_state.rs` | 1,015 | per-doc cycle phase/hash state |
| `ipc_socket.rs` | 414 | Unix-domain-socket IPC |
| **Total** | **~44,516** | |

Adjacent modules likely pulled along (orchestration-side, to confirm during impl): `start`'s agent backends (`agent/`), `terminal.rs`, `focus.rs`, `layout.rs`, `resync.rs`, `hooks.rs`/`hook_cmd.rs`, `project_controller`, `startup_miss`, `gc`, `autoclaim`, `codex_hook`, `ops_log`, `sessions`-adjacent `session_actor*`.

## Dependency structure (intra-cluster edges)

```
sessions      → (leaf)
ipc_socket    → (leaf)
supervisor/   → (leaf — PTY/process only, no cluster deps)
cycle_state   → git              (1 ref — the only back-edge)
git           → capture, cycle_state, ipc_socket
capture       → cycle_state, repair
repair        → capture, cycle_state, git
sync          → capture, cycle_state, git, repair, route, sessions, supervisor
route         → cycle_state, git, repair, sessions, supervisor, sync
start         → cycle_state, sessions, supervisor, sync
```

- **Leaves:** `sessions`, `ipc_socket`, `supervisor/` — no intra-cluster deps.
- **Near-one-way knot:** `git ↔ cycle_state` — `git→cycle_state` is 35 refs, `cycle_state→git` is **1 ref**. That single back-edge is the only true cycle to untangle (or it disappears once both are in the same crate).
- **Mutual pair:** `capture ↔ repair`.
- **Top orchestrators:** `route`, `sync`, `start` depend on nearly everything.

## Extraction strategy — move as ONE crate

Unlike `#adcr` (which split a *layer* with a clean dependency direction), the orchestration cluster has internal cycles (`git↔cycle_state`, `capture↔repair`, `route↔sync`). **Moving these into separate crates is infeasible without breaking the cycles.** Instead, move the **entire cluster into one new workspace member `agent-doc-orchestration`** — internal mutual deps become intra-crate and are a non-issue. The real engineering is the **boundary** between orchestration and the CLI shell.

### What stays in `agent-doc` (CLI shell)

- `main.rs` (clap entrypoint + command dispatch)
- Thin command modules that only parse args and delegate: `claim`, `focus`, `outline`, `history`, `upgrade`, `plugin`, `install`, `commands`, `convert`, `extract`, `undo`, `dedupe`, `clean`, `reset`, `init`, `session_cmd`, `audit_docs`, etc.
- The `ffi.rs` cdylib shell (see FFI below)

### What moves to `agent-doc-orchestration`

- The 10 cluster modules above + their orchestration-adjacent neighbors (confirm the exact set in Wave 1 by following the import graph from `route`/`sync`/`start`).
- Heavy deps move with them: `tokio`, `reqwest`/`ureq`/`hyper`/`rustls`, `interprocess`, `notify`, `rusqlite`, git, `zstd`, `tmux-router`.

## Migration waves (compilable increments)

Because the cluster moves as one crate, "waves" are about migrating in **shim-preserving, always-green increments** (each ends with `make check` + `nm -D` FFI verification), not separate crates:

- **Wave 0 — scaffold.** Create `agent-doc-orchestration` workspace member depending on `agent-doc-core`. Empty lib + `COMPILE_BASELINE.md`.
- **Wave 1 — leaves.** Move `sessions`, `ipc_socket`, `supervisor/`. Leave `pub use agent_doc_orchestration::...` shims in main so the ~100 command modules compile unchanged. (Same shim pattern proven in `#adcr` waves 1–4.)
- **Wave 2 — state/capture core.** Move `cycle_state`, `capture`, `repair` (invert or co-locate the 1 `cycle_state→git` back-edge — likely trivial once `git` moves in Wave 3).
- **Wave 3 — git.** Move `git` (depends on capture/cycle_state/ipc_socket — now all in orchestration).
- **Wave 4 — orchestrators.** Move `route`, `sync`, `start` + neighbors (`terminal`, `layout`, `focus`, `resync`, `hooks`, `project_controller`, `startup_miss`, `gc`, `autoclaim`, `codex_hook`).
- **Wave 5 — prune CLI deps.** Remove now-unused heavy deps from `agent-doc/Cargo.toml`; measure the CLI-shell cold build; update `COMPILE_BASELINE.md`.

Each wave keeps shims in main so `main.rs` dispatch resolves unchanged until a final cleanup pass rewrites call sites to `agent_doc_orchestration::`.

## FFI handling

11 orchestration FFI functions remain in `src/ffi.rs` (`agent_doc_document_changed*`, `agent_doc_sync_*`, `agent_doc_is_tracked`, `agent_doc_set_status`/`get_status`, `agent_doc_*_ipc_listener_*`, `await_idle`/`is_idle`/`is_typing_via_file`, `write_ack_content`, `commit`, `resolve_project_path`).

**Decision: keep ONE cdylib in the `agent-doc` CLI crate.** Editor plugins link a single `libagent_doc.{so,dylib}`. The orchestration FFI bodies move to `agent_doc_orchestration::ffi`, and main re-exports them with `pub use agent_doc_orchestration::ffi::*` + the proven `force_link_*_ffi_symbols` reference pattern (validated across `#k9e1`/`#epv5`/`#vb8h`/`#e130` — all 15 core FFI symbols verified in the export table). No plugin changes; no second cdylib.

## Risks

- **Boundary blast radius.** ~100 command modules and `main.rs` reference `route`/`sync`/`start`/`git`/`sessions`. Shims absorb this during waves; the final call-site rewrite is the largest mechanical step.
- **`project_controller` / `session_actor` / `startup_miss` / `gc` coupling.** These cross-cut orchestration and may need to move together; confirm the import graph in Wave 1 before committing the module set.
- **cdylib symbol preservation.** Mitigated by the `force_link` pattern (proven). Every wave touching FFI must re-run `nm -D libagent_doc.so`.
- **`tmux-router` patch.** The superproject `.cargo/config.toml` `[patch.crates-io]` for `tmux-router` must follow the crate that depends on it (orchestration).
- **Test migration.** Each module's inline + integration tests move with it; the `tests/` integration suite (incl. the known-flaky live-tmux `codex_bare_run_inside_owning_pane` test) targets orchestration behavior and may need the dep on `agent-doc-orchestration` as a dev-dependency.
- **Build-time cost.** Multi-cycle (5+ waves). Largest agent-doc refactor since the original Wave 4.

## Acceptance criteria

1. `agent-doc-orchestration` compiles standalone and depends on `agent-doc-core`.
2. The `agent-doc` CLI shell builds **without** `tokio`/`hyper`/`reqwest`/`rustls`/`interprocess`/`notify`/`rusqlite`/`git2`/`zstd` as direct deps (they reach it only transitively through orchestration, or not at all for pure-CLI commands).
3. CLI-shell cold build measured and recorded in `COMPILE_BASELINE.md` (target: materially below the 129s full build).
4. `cargo test` green across all three crates; `make check` green (modulo the pre-existing live-tmux environmental test).
5. `nm -D libagent_doc.so` still exports the full FFI surface (core + orchestration).

## Decomposition into implementation sub-tasks

The actual extraction is tracked as `#adoc-orch-*` sub-tasks (filed on backlog, referencing this plan), one per wave: `#adoc-orch-scaffold`, `#adoc-orch-leaves`, `#adoc-orch-state`, `#adoc-orch-git`, `#adoc-orch-orchestrators`, `#adoc-orch-prune`. Each is an independent green-to-green cycle with its own `make check` + `nm -D` gate, mirroring the `#adcr` wave cadence.
