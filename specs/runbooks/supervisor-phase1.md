---
name: supervisor-phase1
status: draft
date: 2026-04-13
parent: ../supervisor.md
---

# Supervisor Phase 1 Implementation Playbook

PR-by-PR checklist for landing the `agent-doc start` supervisor. Design rationale and invariants live in [../supervisor.md](../supervisor.md) — this runbook is the execution track. If a step here conflicts with the design spec, the design spec wins; fix this file.

## Ordering invariant

PRs land in the order below. Each depends on the previous. Do NOT reorder without updating the spec's "Phase 1 Implementation Status" table.

```
cwd.rs  →  pty.rs  →  env.rs  →  resize.rs  →  state.rs  →  ipc.rs  →  start.rs wire-up
```

Rationale: each layer is independently testable against a fake-claude shell script. `start.rs` is last because it is the only step visible in production — everything before it sits behind `#![allow(dead_code)]` in `supervisor/mod.rs`.

## Shared conventions

Apply to every submodule PR in this track:

- **Module doc comment pins invariants.** The first thing in `supervisor/<mod>.rs` is a `//!` block listing the non-obvious constraints. Future refactors read this before touching the file.
- **Fake-claude tests only.** Integration tests spawn a shell script inside a `TempDir`, never real `claude`. Real-claude smoke testing is deferred to the `#f7d5` backlog item.
- **`#![allow(dead_code)]` gate.** `supervisor/mod.rs` carries a module-level `#![allow(dead_code)]` until the `start.rs` wire-up PR removes it. Each intermediate PR must compile clean with the gate ON.
- **Never swallow errors.** Per `src/agent-doc/CLAUDE.md` — any fallible op logs to stderr at minimum. No `let _ =`.
- **No new async runtime.** Std threads + blocking I/O, same pattern as `ipc_socket.rs`.
- **`make check` green before PR.** clippy + test, no exceptions.

## Stage 0 — cwd.rs ✅ LANDED

Reference only. Do not re-land.

- Priority chain: `--cwd` flag > `agent_doc_cwd` frontmatter > `.agent-doc/` walk > document parent.
- Relative-path bases differ by source (CLI → invocation cwd, frontmatter → doc parent).
- Misconfigured paths hard-error with source-labelled context.
- `CwdSource::as_str()` tags locked by `source_tag_strings_are_stable` test.
- `find_project_root` duplicated rather than reused from `snapshot.rs`.
- 11 unit tests, clippy clean.

## Stage 1 — pty.rs ✅ LANDED

Reference only.

- `PtySpawnConfig { program, args, cwd, env, size }` — caller pre-resolves everything.
- `PtySession::spawn(cfg)` — `env_clear()` then re-populate, slave dropped immediately after spawn.
- `PtySession::forward_stdio()` — separate from spawn so tests can skip I/O threads.
- `PtySession::resize(size)` / `wait()` / `kill()` — thin wrappers for sibling modules.
- Parent env is **not** inherited (the cascade lives in `env.rs`, one layer up).
- 6 targeted tests locking: clean exit, nonzero exit, env-not-inherited, cwd set, resize, missing program.
- `portable-pty = "0.8"` added to `Cargo.toml`.

## Stage 2 — env.rs ✅ LANDED

Reference only.

- Schema lift: `Frontmatter.env: IndexMap<String, Option<String>>` (supports `KEY: null` to unset).
- `env::expand_values` and `shell_export_prefix` take the new map type; `unset KEY` emitted for `None`.
- `supervisor::env::EnvSpec { inherit_parent, overrides }` — thin resolver over the shared `agent_doc_config::env` foundation.
- `resolve()` captures `std::env::vars()` **once** (when `inherit_parent`) so `state.rs` restarts are deterministic.
- `agent_doc_env_inherit` frontmatter field (default `true`).
- Consumers (`start.rs`, `run.rs`, `stream.rs`, `parallel.rs`, `preflight.rs`) updated to handle `None`.

## Stage 3 — resize.rs

**Pending ID:** `#jg0d`

### Scope
- `ResizeWatcher` trait with two platform impls: `#[cfg(unix)]` and `#[cfg(windows)]`.
- Unix: `signal-hook` SIGWINCH → `crossbeam_channel` → resize thread → `TIOCGWINSZ` → `PtySession::resize`.
- Windows: stub for phase 1 (WSL handles the real case via Unix path). Log a warning and no-op.

### Module layout
```
supervisor/resize.rs
  pub trait ResizeWatcher { fn start(&self, pty: Arc<Mutex<PtySession>>) -> Result<JoinHandle<()>>; }
  mod platform_unix { ... }   // #[cfg(unix)]
  mod platform_windows { ... } // #[cfg(windows)]
  pub fn watcher() -> impl ResizeWatcher { ... }
```

### Invariants to pin in module doc comment
1. **Initial size pushed before the watcher blocks.** The first resize call fires synchronously so claude boots at the real pane size, not the 24×80 default from `PtySpawnConfig::new`.
2. **Watcher is tear-down-safe.** On `PtySession` drop, the channel closes and the thread exits via `recv()` error. Never join-leak.
3. **TIOCGWINSZ source is stdin.** The supervisor reads the pane size from its own stdin fd (which is the tmux pane's pty), not from the child pty. Getting this wrong means claude resizes to its own size, not the pane's.
4. **Windows stub logs once.** Warn on first call, then stay silent. Do not spam logs on every SIGWINCH-equivalent event.

### Tests
| Test | Locks |
|------|-------|
| `unix_sigwinch_triggers_resize` | Fake stdin with known `TIOCGWINSZ` values; send SIGWINCH; assert `PtySession::resize` called with those values |
| `initial_resize_fires_before_watcher_blocks` | Spawn watcher, assert first resize call happened synchronously |
| `watcher_exits_cleanly_on_pty_drop` | Drop the `Arc<PtySession>`, assert `JoinHandle::join` returns within 100ms |
| `windows_stub_warns_once` (`#[cfg(windows)]`) | Call resize path N times, assert exactly one warning line |

### Cargo.toml
- `signal-hook = "0.3"` (unix only, behind `#[cfg(unix)]` in `Cargo.toml` `[target]` block)
- `crossbeam-channel = "0.5"` — already a transitive dep, pin if missing

### Done criteria
- All 4 tests green.
- `cargo check --target x86_64-pc-windows-gnu` compiles the stub (CI check, optional locally).
- `supervisor/mod.rs` still carries `#![allow(dead_code)]`.

## Stage 4 — state.rs

**Pending ID:** `#b486`

### Scope
- Crash classifier: exit code + timestamp → `ExitClass` (`Clean`, `Transient`, `Flapping`).
- Ring buffer (`VecDeque<(Instant, i32)>`, cap 10) for `exits_in_last_60s`.
- State machine: `Healthy → Transient → Flapping → Halted` per spec §"Crash Recovery Policy".
- `state.rs` owns the restart loop — consumes `PtySession`, calls `env.rs`/`cwd.rs` on each restart.

### Module layout
```
supervisor/state.rs
  pub enum SupervisorState { Healthy, Degraded, Halted }
  pub enum ExitClass { Clean, Transient, Flapping }
  pub struct RestartHistory { buffer: VecDeque<...>, cap: usize }
  pub struct StateMachine { state, history, consecutive_failures, cfg }
  impl StateMachine {
      pub fn classify(&self, exit_code: i32) -> ExitClass;
      pub fn on_exit(&mut self, exit_code: i32) -> Action; // sleep_secs, restart_mode, new state
  }
  pub enum Action { PromptUser, Restart { sleep_secs, mode }, Halt }
```

### Invariants to pin
1. **Time source is `Instant::now()`, not wall clock.** Flap detection must be monotonic; wall-clock jumps (NTP, suspend/resume) cannot spuriously clear the flap counter. Tests inject a `Clock` trait to avoid real sleeps.
2. **`consecutive_failures` resets on `Clean` only.** A `Transient` between `Flapping` states does NOT reset the counter — otherwise a flapping child that occasionally exits 0 could loop forever.
3. **`Halted` is terminal.** Only explicit user action (`supervisor resume` CLI) can exit `Halted`. The state machine never self-recovers.
4. **Ring buffer cap is 10, window is 60s.** Configurable via `StateMachineConfig`, but defaults are locked by tests so tuning is explicit.

### Tests (via injected `Clock`)
| Test | Locks |
|------|-------|
| `clean_exit_transitions_healthy` | `exit 0` → `Action::PromptUser`, state `Healthy` |
| `single_transient_restarts_with_2s` | `exit 1` → `Action::Restart { sleep_secs: 2, mode: continue }` |
| `three_exits_in_60s_classifies_flapping` | Three `exit 1` within 60s → `ExitClass::Flapping`, 30s sleep |
| `five_consecutive_flapping_halts` | Five consecutive flapping exits → `Action::Halt`, state `Halted` |
| `transient_between_flapping_does_not_reset_counter` | Sequence: flap, flap, transient, flap, flap, flap → still halts at 5 |
| `clean_resets_counter` | flap × 4 → clean → flap × 4 → not halted yet |
| `wall_clock_jump_does_not_affect_flap_detection` | Move injected clock backward 1h → flap counter unchanged |
| `ring_buffer_caps_at_10` | Push 20 exits → `buffer.len() == 10`, oldest dropped |

### Done criteria
- All 8 tests green.
- No real `std::thread::sleep` calls in tests — all via `Clock` trait.
- Module public surface documented for `ipc.rs` consumption (the `state` IPC method returns `SupervisorState`).

## Stage 5 — ipc.rs

**Pending ID:** `#40ct`

### Scope
- Unix-domain socket accept loop at `.agent-doc/supervisor/<session-uuid>.sock`, mode `0600`.
- Reuse length-prefixed JSON frame format from `ipc_socket.rs`.
- Five methods per spec §"IPC Socket": `restart`, `inject`, `state`, `pid`, `stop`.

### Module layout
```
supervisor/ipc.rs
  pub struct IpcServer { socket_path, state_handle: Arc<Mutex<StateMachine>>, pty_handle: Arc<Mutex<PtySession>> }
  impl IpcServer {
      pub fn bind(session_uuid: &str, ...) -> Result<Self>;
      pub fn run(self) -> JoinHandle<()>; // accept loop thread
  }

  #[derive(Serialize, Deserialize)] enum Request { Restart{mode}, Inject{bytes}, State, Pid, Stop{graceful} }
  #[derive(Serialize, Deserialize)] enum Response { Ok{...}, Err{msg} }

  fn handle_connection(stream, state, pty) -> Result<()>;
```

### Invariants to pin
1. **Socket file is cleaned up on normal exit and on `stop`.** Stale detection via `connect()` in `register()` — if `ECONNREFUSED` and the controller-recorded supervisor PID is dead, unlink and rebind.
2. **One request per connection.** Keep the protocol request/response only — no long-lived subscriptions in phase 1. Simplifies the accept loop and avoids state leaks.
3. **Accept loop is terminatable via `stop` request.** A `stop` message closes the listener and joins the accept thread. No `SIGTERM` dance required.
4. **`inject` does not block on pty backpressure.** Write with a 100ms timeout; return `Err` if the pty master is full. Otherwise a slow child blocks the IPC thread.
5. **All handlers lock `state` and `pty` in the same order.** `state` first, then `pty`. Prevents deadlocks with the restart loop in `state.rs`.

### Tests
| Test | Locks |
|------|-------|
| `bind_creates_socket_with_mode_0600` | Stat the socket file, assert mode |
| `stale_socket_is_cleaned_on_rebind` | Create a dangling socket file, call `bind`, assert it rebinds |
| `state_request_returns_current_state` | Drive `StateMachine` into `Degraded`, send `state`, parse response |
| `restart_request_kills_and_respawns` | Fake-claude, send `restart`, assert new pid in response |
| `inject_writes_to_pty` | Send `inject` with `"hello\n"`, assert fake-claude's stdout contains "hello" |
| `stop_request_shuts_accept_loop` | Send `stop`, assert accept thread joins within 500ms |
| `concurrent_requests_do_not_deadlock` | Two clients, one `state` + one `restart`, both complete |
| `inject_timeout_returns_err` | Fake-claude that never reads stdin, send large payload, assert `Err` response within 150ms |

### Done criteria
- All 8 tests green.
- Lock order documented in module doc comment.
- `ipc.rs` does not depend on `start.rs` — `start.rs` composes it.

## Stage 6 — start.rs wire-up

**Pending IDs:** `#vnp0` (replace existing restart loop), `#6ae3` (pty), `#zp02` (env)

### Scope
This is the **only PR visible in production**. Replaces the existing `start.rs:170` restart loop with `supervisor::run(file, opts)`. Removes `#![allow(dead_code)]` from `supervisor/mod.rs`. Deletes the old sleep-2s + `--continue` path.

### Order of operations
1. **Compose the supervisor.** `supervisor::run(file, opts)` does:
   ```rust
   let fm = frontmatter::read(file)?;
   let resolved_cwd = cwd::resolve(file, &fm, opts.cwd_flag.as_deref())?;
   let env_spec = env::EnvSpec::from_frontmatter(&fm);
   let resolved_env = env_spec.resolve()?;
   let size = resize::initial_size()?;
   let pty_cfg = PtySpawnConfig { program, args, cwd: resolved_cwd.path, env: resolved_env, size };
   let pty = Arc::new(Mutex::new(PtySession::spawn(pty_cfg)?));
   let state = Arc::new(Mutex::new(StateMachine::new(StateMachineConfig::default())));
   let ipc = IpcServer::bind(&session_uuid, state.clone(), pty.clone())?;
   let resize_handle = resize::watcher().start(pty.clone())?;
   let ipc_handle = ipc.run();
   state::run_restart_loop(pty, state, &env_spec, &resolved_cwd)?;
   // on exit: drop pty, join resize + ipc, unlink socket
   ```
2. **Delete `start.rs:170-290`** — the old restart loop, sleep-2s path, and manual `--continue` logic.
3. **Remove `#![allow(dead_code)]`** from `supervisor/mod.rs`. Clippy will now enforce the full public surface.
4. **Update the `[supervisor]` log tag paths** — every log line from the new modules uses the tag prefix per spec §"Logging".
5. **Keep the entry gate.** `sessions::in_tmux()` still hard-errors outside tmux. Non-tmux mode is future work.
6. **Keep the session-log contract.** `.agent-doc/logs/<session-uuid>.log` remains the single log file. Supervisor events append via `ops_log.rs` or a dedicated writer.

### Invariants to pin
1. **One `supervisor::run` call per `agent-doc start` invocation.** No re-entry, no nested supervisors.
2. **Clean shutdown order is reverse of startup.** Kill claude → drop pty → close IPC socket → join threads → unlink socket file. Any other order risks leaked sockets or threads.
3. **`AGENT_DOC_SESSION` and `AGENT_DOC_DOCUMENT` env vars set via `EnvSpec` overrides**, not in `pty.rs`. Keeps `pty.rs` ignorant of session semantics.
4. **Release notes flag the behavior change.** In-flight sessions are unaffected; next `agent-doc start` uses the supervisor.
5. **SKILL.md `cd` fix (if shipped) becomes redundant but harmless.** Do not delete it in the same PR — separate cleanup.

### Tests
| Test | Locks |
|------|-------|
| `start_launches_supervisor_with_fake_claude` | End-to-end with a fake-claude script; supervisor exits cleanly on `exit 0` |
| `start_respects_cwd_frontmatter` | Frontmatter `agent_doc_cwd: ./sub`; fake-claude asserts `pwd` matches |
| `start_respects_env_frontmatter_unset` | Frontmatter `env: { PARENT_VAR: null }`; fake-claude asserts var unset |
| `ipc_socket_exists_after_start` | Connect to `<session>.sock` during run, issue `state`, get response |
| `clean_exit_unlinks_socket` | After clean exit, assert socket file gone |
| `flapping_child_halts_supervisor` | Fake-claude that always `exit 1`; assert supervisor reaches `Halted` and exits with a diagnostic |

### Done criteria
- All 6 end-to-end tests green.
- `#![allow(dead_code)]` removed.
- `make check` + `make install-full` + manual smoke test per release checklist.
- `src/agent-doc/VERSIONS.md` entry drafted.
- Release notes draft mentions the behavior change.

## Post-phase-1 follow-ups

These do NOT block the phase 1 landing. Track separately in `agent-doc-bugs.md` pending.

- `#f7d5` — real-claude smoke test in a live tmux pane (manual testing gate).
- Future non-tmux mode per supervisor spec §"Non-tmux Mode (Future)".
- Windows ConPTY resize watcher (real impl, not stub) — only if a consumer demands it.
- `supervisor resume <session>` CLI for `Halted` recovery — defer until the first real flap is observed.

## Pending-item cross-reference

| Stage | Pending ID | Status |
|-------|-----------|--------|
| 0. cwd.rs | (landed, no pending) | ✅ |
| 1. pty.rs | (landed, no pending) | ✅ |
| 2. env.rs | (landed, no pending) | ✅ |
| 3. resize.rs | `#jg0d` | pending |
| 4. state.rs | `#b486` | pending |
| 5. ipc.rs | `#40ct` | pending |
| 6. start.rs wire-up | `#vnp0`, `#6ae3`, `#zp02` | pending |
| Smoke test | `#f7d5` | pending |

Updating this table is part of each stage's "Done criteria" — if the pending IDs in `agent-doc-bugs.md` drift, fix this table in the same PR.
