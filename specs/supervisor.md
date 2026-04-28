---
name: supervisor
status: draft
date: 2026-04-13
---

# agent-doc Supervisor Spec

## Motivation

`agent-doc start` is a supervisor-lite: it wraps `claude` in a restart loop inside a tmux pane (see `src/start.rs:170`). It handles clean-exit prompts and non-zero auto-restart, but:

- **Has no pty** — `claude` inherits the tmux pane's tty directly. The supervisor cannot observe output, inject keystrokes, or detect prompt state.
- **CWD is inherited from the tmux pane** — whatever directory the pane happens to be in at spawn time, which is the root cause of the cross-project CWD drift bug this spec exists to fix.
- **Has no IPC** — external processes (editor plugins, `/agent-doc` routing from other panes) can only talk to the running claude via `tmux send-keys`, which is racy and cannot introspect state.
- **Crash recovery is a 2s sleep + `--continue`** — no escalation, no state inspection, no cooldown.

The supervisor graduates `start.rs` into a process that **owns** claude as a child behind a pty, holds a Unix-domain IPC socket per session, and enforces invariants (CWD, env, auto-restart cadence, external control) that a bare tmux-pane wrapper cannot.

## Phase 1 Implementation Status

| Submodule | Status | Notes |
|-----------|--------|-------|
| `supervisor/cwd.rs` | **landed** | Priority chain + 11 unit tests + frontmatter `agent_doc_cwd` field. Design invariants pinned below under Core Invariants / CWD determinism. |
| `supervisor/pty.rs` | not started | `portable-pty` allocation, stdin→pty and pty→stdout forwarding threads, fake-claude shell-script integration test. |
| `supervisor/resize.rs` | not started | Unix SIGWINCH path via `signal-hook`; Windows ConPTY path deferred (WSL handles phase 1). |
| `supervisor/state.rs` | not started | Crash classifier, ring buffer, Healthy→Transient→Flapping→Halted state machine. |
| `supervisor/ipc.rs` | not started | Unix-domain socket accept loop, length-prefixed JSON frame reuse from `ipc_socket.rs`. |
| `start.rs` wire-up | not started | Replace existing restart loop with thin wrapper around `supervisor::run`. Last PR in the phase 1 sequence — see Migration from `start.rs`. |

Phase 1 lands in sequential PRs in this order, so each layer can be tested against the real binary before the next lands. `start.rs` is deliberately last: until it switches over, the supervisor module compiles but is not yet consumed in production, and the module-level `#![allow(dead_code)]` in `supervisor/mod.rs` suppresses warnings during the intermediate commits.

## Non-Goals

- Not a tmux replacement. Supervisor still runs inside a tmux pane; tmux still owns the visual terminal the user sees.
- Not a sandbox. Supervisor does not restrict what claude can do — it just owns its lifecycle.
- Not a daemon for multiple sessions. **One supervisor = one claude = one session document.** Per-session daemons are easier to reason about than a global one.
- Not a replacement for `ipc_socket.rs` — that socket handles write-path IPC between the binary and editor plugins. Supervisor IPC is a different socket, scoped to claude lifecycle control.

## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│ tmux pane                                                    │
│                                                              │
│  ┌────────────────────────────────────────────────────┐     │
│  │ agent-doc start (supervisor process)               │     │
│  │                                                    │     │
│  │   ┌──────────────────────────────────────────┐    │     │
│  │   │ pty master  ←→  claude (child)           │    │     │
│  │   │               (pty slave = claude's tty) │    │     │
│  │   └──────────────────────────────────────────┘    │     │
│  │                                                    │     │
│  │   stdin  ─(copy)──→ pty master                     │     │
│  │   pty master ─(copy)──→ stdout                     │     │
│  │                                                    │     │
│  │   Unix socket: .agent-doc/supervisor/<session>.sock│     │
│  │     ├── POST /restart                              │     │
│  │     ├── POST /inject <bytes>                       │     │
│  │     ├── GET  /state                                │     │
│  │     └── GET  /pid                                  │     │
│  └────────────────────────────────────────────────────┘     │
└──────────────────────────────────────────────────────────────┘
```

The supervisor is a single process that:
1. Allocates a pty pair (master/slave) via `portable-pty` or `nix::pty`.
2. Forks claude with the slave as its tty, with a deterministic CWD and env.
3. Runs three threads: stdin→pty, pty→stdout, IPC socket accept loop.
4. Wait-loop on the child; on exit, applies restart policy.

## Core Invariants

### CWD determinism
- Supervisor resolves the target CWD **once at startup** from (priority order):
  1. `--cwd <path>` CLI flag
  2. Frontmatter `agent_doc_cwd: <path>` (new field)
  3. Project root resolved by walking up from the document until `.agent-doc/` is found
  4. Document's parent directory (fallback)
- The resolved CWD is set on the claude child process via `Command::current_dir`.
- The resolved CWD is **also** `cd`'d into at the pty level before claude spawns, so any shell-side state inside claude (e.g., `pwd` in bash tools) sees the same directory.
- On restart (`--continue` or fresh), the CWD is re-resolved — not cached — so moving the document mid-session is safe.

**Implementation invariants** (pinned by `supervisor/cwd.rs` unit tests):

- **Relative-path bases differ by source.** `--cwd` relative paths are resolved against the supervisor's invocation CWD (standard `Path::canonicalize` behavior). `agent_doc_cwd` relative paths are resolved against the **document's parent directory**, so a frontmatter value of `..` means "parent of the folder the document lives in," independent of wherever the user happened to invoke `agent-doc start` from. Absolute paths bypass the base in both cases.
- **Misconfigured paths hard-error, never silently fall through.** If `--cwd /bogus` or `agent_doc_cwd: nowhere` points at a nonexistent path or a non-directory, the resolver returns an error with a source-labelled context string (`--cwd flag: path does not exist: ...` / `agent_doc_cwd frontmatter: path is not a directory: ...`). Silent fall-through to a lower priority would mask typos and make cross-project CWD drift — the exact class of bug this spec exists to fix — harder to debug.
- **Document existence is a precondition.** The resolver canonicalizes the document path first; a missing document is a hard error with a clear message, not a fallback-to-cwd.
- **Self-contained module.** `supervisor::cwd::find_project_root` is duplicated rather than reused from `snapshot::find_project_root`. The supervisor process runs at a lifecycle layer below snapshot management and must not pull in that dependency graph; the walk is a six-line loop and is exercised by the `deeply_nested_document_still_finds_project_root` test.
- **Source tagging for logs/IPC.** `CwdSource` exposes stable `as_str()` tags (`cli_flag`, `frontmatter`, `project_root`, `document_parent`) — the log line `cwd_resolved source=<tag>` and the IPC `state` response use these verbatim. The `source_tag_strings_are_stable` test locks the tag strings so downstream tooling doesn't break on refactors.

### Env determinism
- Supervisor builds the child env from:
  1. Parent process env (whitelisted via `HOME`, `PATH`, `TERM`, `LANG`, `TMUX`, `USER`, …)
  2. Frontmatter `env:` map (already expanded by `crate::env::expand_values`)
  3. `AGENT_DOC_SESSION=<uuid>` and `AGENT_DOC_DOCUMENT=<path>` (new, so claude can introspect its own session without parsing frontmatter)
- **No inheritance of `PWD` from the tmux pane** — supervisor sets `PWD` explicitly to match the resolved CWD, so shell-side `$PWD` matches the process-level CWD.

### Pty lifecycle
- Pty is allocated before claude spawns and destroyed after claude exits + IPC socket closes.
- SIGWINCH on the tmux pane → forwarded to the pty master so claude sees resize.
- On supervisor exit (user `q`), pty slave closes, claude gets SIGHUP.

## IPC Socket

Per-session Unix-domain socket at `.agent-doc/supervisor/<session-uuid>.sock`.

Protocol: length-prefixed JSON (same frame format as `ipc_socket.rs`, so the existing client code in FFI can be reused).

### Methods

| Method | Request | Response | Notes |
|--------|---------|----------|-------|
| `restart` | `{ "mode": "fresh" \| "continue" }` | `{ "ok": true, "pid": <u32> }` | Kills current claude, relaunches |
| `inject` | `{ "bytes": "<base64>" }` | `{ "ok": true, "n": <usize> }` | Write bytes to pty master |
| `state` | — | `{ "running": bool, "pid": u32?, "cwd": string, "restart_count": u32, "last_exit": i32? }` | |
| `pid` | — | `{ "pid": u32? }` | Convenience shortcut |
| `stop` | `{ "graceful": bool }` | `{ "ok": true }` | Shuts down supervisor + child |

Socket is created with mode `0600`. Opaque to anything except the FFI library, which exposes typed C ABI wrappers.

### External control use cases

1. **`/agent-doc` routing from a different tmux pane:** instead of `tmux send-keys`, the route subcommand opens the supervisor socket and calls `inject` with `/agent-doc <file>\r`. Removes the 5s sleep hack + race conditions in `start.rs:229`.
2. **Editor plugin "restart claude" button:** IntelliJ plugin opens the socket, calls `restart`, displays the returned pid.
3. **Crash-state introspection for health dashboards:** `state` returns last exit code + restart count so a cleanup hook can escalate (e.g., "5 restarts in 60s → stop and notify").

Both `route` and `resync` also use the supervisor socket's `Pid` method as a secondary ownership proof when tmux argv/path inspection no longer proves which pane owns a registered document. That fallback maps the live child PID back to the tmux pane before treating the registration as stale.

## Crash Recovery Policy

Replaces the current "sleep 2s + `--continue`" with a state machine:

```
state = Healthy
on claude exit with code c:
    append to restart history (ring buffer, last 10 exits with timestamps)
    classify:
        c == 0  → Clean
        c != 0 AND exits_in_last_60s < 3 → Transient
        c != 0 AND exits_in_last_60s >= 3 → Flapping
    action:
        Clean:    harness-specific clean-exit handling, transition Healthy
                  Claude: prompt user (Enter/q)
                  Codex: auto-restart in resume mode so `codex exec` stays attached
                         EXCEPT when stdin EOF (Ctrl+D) detected → restart fresh
                         instead of resuming so the pane stays attached
                         AND resume auto-trigger only accepts a prompt line that
                         appears as the latest non-empty line in the current
                         resumed child's filtered pty output; stale tmux history
                         is not enough
                         AND when the resumed child never re-establishes a prompt
                         (`auto_trigger_timeout` / child-pty trigger failure), treat the
                         handoff as failed provenance. The 30s
                         `auto_trigger_timeout` log is provisional telemetry:
                         the watcher keeps polling after that point, and the
                         timeout only remains terminal if the child exits
                         without a later prompt/send success:
                         - first failure in the 15-minute window → restart fresh
                           instead of chaining another blind `resume --last`
                         - second failure in the 15-minute window → stop the
                           blind loop and prompt the user (Enter fresh / q exit)
        Transient: sleep 2s, restart with --continue, state Healthy
        Flapping:  sleep 30s, restart with --continue, state Degraded
                   on 5th consecutive failure → state Halted
        Halted:   do not restart. Print "supervisor halted — run 'agent-doc
                  supervisor resume <session>' to retry"
```

State is surfaced via `state` IPC method so dashboards / cleanup hooks can observe it.

## Migration from `start.rs`

**Decided: `agent-doc start` always runs the supervisor.** No feature flag, no phased rollout.

- The existing `start.rs` restart loop is deleted and replaced by a thin wrapper around `supervisor::run`.
- One PR lands the supervisor + removes the old loop simultaneously.
- Release notes call this out as a behavior change for in-flight sessions — existing claude processes are unaffected (the supervisor only owns processes it spawned); the behavior change lands the next time the user runs `agent-doc start`.
- SKILL.md `cd` fix: if already shipped by then, it becomes redundant but harmless. If not yet shipped, it can be skipped — supervisor CWD resolution subsumes it.

## Five Hard Parts — Answers

From the previous exchange:

1. **Pty vs. raw inheritance** — pty. Use `portable-pty` crate for cross-platform (we need at least Linux + macOS for dev + manylinux for PyPI builds). The pty allows stdin/stdout forwarding while giving us an injection point.

2. **CWD determinism** — resolved at supervisor startup from CLI flag > frontmatter > project root > doc parent. Set both on the child process and inside the pty via a `cd` command before handing control to claude.

3. **Crash recovery** — state machine with ring buffer (Healthy → Transient → Flapping → Halted). Flap detection via `exits_in_last_60s`.

4. **External control** — per-session Unix socket, length-prefixed JSON, four methods (restart, inject, state, stop). Reuses frame format from `ipc_socket.rs`.

5. **IPC lifecycle** — supervisor owns the socket for its lifetime. Socket file is cleaned up on normal exit and on `stop`. Stale sockets are detected by connecting during `register()` — if connect fails with ECONNREFUSED and the pid in `sessions.json` is dead, delete the stale socket.

## Resize Handling

**Unix (Linux/macOS):** `signal-hook` installs a SIGWINCH handler that pushes an event into a `crossbeam_channel`. A small resize thread blocks on the channel, queries `TIOCGWINSZ` on stdin fd, and calls `pty_master.resize(PtySize { rows, cols, .. })` from `portable-pty`.

**Windows:** no SIGWINCH. Resize comes from the console input queue as `WINDOW_BUFFER_SIZE_EVENT`. Options, in order of preference:

1. **`portable-pty::native_pty_system()` already abstracts ConPTY** — on Windows, `PtyMaster::resize()` calls `ResizePseudoConsole`. We need to **feed** resize events into that call ourselves.
2. **Source of resize events on Windows:** `ReadConsoleInputW` on `stdin` handle returns a `WINDOW_BUFFER_SIZE_EVENT` record whenever the console window resizes. Spawn a thread that loops on `ReadConsoleInputW`, filters for that event type, and calls `pty_master.resize()`.
3. Since `agent-doc start` already requires tmux (`sessions::in_tmux()` at `start.rs:128`), and tmux on Windows means **WSL**, in practice the Windows path is just "Linux in WSL" — SIGWINCH works normally. A pure-Win32 build path is only relevant if we ever support non-tmux sessions on Windows, which this spec does not.

**Resolution:** `#[cfg(unix)]` uses SIGWINCH + `signal-hook`. `#[cfg(windows)]` uses `ReadConsoleInputW` in a dedicated thread. Both feed into the same `pty_master.resize()` sink. The `resize.rs` submodule has two `mod platform_{unix,windows}` implementations with a common `ResizeWatcher` trait.

**WSL caveat:** the realistic "Windows support" story for `agent-doc start` is WSL, because tmux itself is not native to Windows. The ConPTY code path exists for future-proofing non-tmux Windows sessions (e.g., a future `agent-doc start --no-tmux` running claude directly in Windows Terminal), not for phase 1 shipping.

## Non-tmux Mode (Future)

The supervisor is architecturally independent of tmux — it owns claude behind its own pty, so the outer terminal the user sees is conceptually separable from how claude is wrapped. Phase 1 still requires tmux (see Non-Goals), but a future `--no-tmux` mode is reachable by changing four things:

1. **Entry gate.** `start.rs:128` currently calls `sessions::in_tmux()` and refuses to run outside a tmux pane. Becomes a mode check (`--tmux` default, `--no-tmux` opt-in).
2. **Resize source.** Today SIGWINCH originates from the tmux pane (see `resize.rs` unix path). Without tmux, the source becomes the outer controlling tty directly on Linux/macOS, and `ReadConsoleInputW` (`WINDOW_BUFFER_SIZE_EVENT`) on native Windows. The ConPTY scaffolding already called out in the Resize Handling section covers this case.
3. **IPC discoverability.** The socket path keys off session id, not tmux, so nothing in `ipc.rs` needs to change. What *does* change is how *other* processes find the socket: today "`/agent-doc` routing from a different tmux pane" (see Use Cases) assumes a tmux pane id; outside tmux the discovery key becomes session id or pid.
4. **Claim / binding model.** `agent-doc claim` binds a document to a tmux pane id. With no panes, the binding unit becomes something else — pid, socket path, or tty name. This is the biggest conceptual change, not a technical one, and needs its own spec (what does "focus" mean without tmux? what does `agent-doc focus` do?).

**Non-goals for this future mode:** visual multiplexing. If a user wants multiple simultaneous claude sessions visible at once, tmux (or an equivalent multiplexer) is still the right tool. `--no-tmux` is for single-session use cases — typically a developer running one claude in their terminal without a multiplexer layer, or an embedded context (editor terminal, CI, container entrypoint) where tmux is unavailable or undesirable.

**Not in phase 1.** Every current consumer (livestream setup, `/agent-doc` cross-pane routing, the claim system, `agent-doc focus`) assumes tmux. Dropping tmux requires re-specifying those consumers, which is a separate design cycle. This section exists so the four requirements above are not lost when phase 1 ships.

## Logging

Single log file per session at `.agent-doc/logs/<session-uuid>.log`, same path as today. Supervisor events use a `[supervisor]` tag prefix on each line for filtering. Format: `[<epoch>] [supervisor] <event> key=value ...`.

Example:
```
[1713041234] session_start file=tasks/plan.md pane=%12 session=abc12345
[1713041234] [supervisor] pty_allocated rows=40 cols=120
[1713041234] [supervisor] cwd_resolved path=/home/brian/work/agent-loop source=project_root
[1713041234] [supervisor] claude_spawn pid=54321 mode=fresh
[1713041290] [supervisor] claude_exit code=0
[1713041291] [supervisor] user_action=restart
[1713041291] [supervisor] claude_spawn pid=54398 mode=continue
[1713041390] [supervisor] codex_exit code=0
[1713041390] [supervisor] auto_restart_clean with_continue=true
[1713041391] [supervisor] codex_spawn pid=54444 mode=continue
[1713041421] [supervisor] auto_trigger_timeout pane=%12 harness=codex reason=no_prompt_after_30s
[1713041425] [supervisor] resume_restart_failed pane=%12 harness=codex outcome=timeout recent_failures=1 window_secs=900 restart_count=1
```

Auto-trigger provenance is lifecycle-bound to a single restart iteration:
- each restart spawns at most one auto-trigger thread
- when the child exits, that thread is explicitly cancelled and joined before
  the next restart iteration begins
- if a resumed child exits cleanly before the auto-trigger ever sends, that
  still counts as a failed resume handoff; clean exit alone does not clear the
  failed-resume guard
- stale auto-trigger workers must never outlive the child they were waiting on,
  so they cannot inject commands into a later replacement child in the same pane
- after the prompt appears, the trigger command is written through the
  supervisor-owned child pty writer rather than tmux pane stdin, so a late
  trigger cannot hit the supervisor restart prompt or a non-child process in
  the pane
- supervisor shutdown signals both the auto-trigger stop flag and the
  stdin->pty writer stop path before joining either thread, and the shared
  child-pty writer lock/write path is cancellation-aware, so a stop request
  does not hang behind a blocked stdin->pty writer mutex wait

This keeps the existing `.agent-doc/logs/<session>.log` contract intact for any downstream tooling (`agent-doc logs`, dashboards) and avoids a second log file to rotate.

## Dependencies to Add

- `portable-pty` — pty allocation. Supports Unix pty + Windows ConPTY under one API, which we need because Windows is a supported target.
- `signal-hook` (`#[cfg(unix)]`) — SIGWINCH handler on Unix.
- `winapi` or `windows-sys` (`#[cfg(windows)]`) — `ReadConsoleInputW` for resize events, only for future non-tmux Windows sessions. Phase 1 can stub the Windows resize watcher since WSL handles the tmux case via Unix SIGWINCH.
- No new async runtime — the IPC socket accept loop runs in a std thread, same pattern as `ipc_socket.rs`.

## Testing Strategy

- **Unit:** CWD resolution priority, env whitelist, crash classifier, flap detection.
- **Integration:** spawn supervisor with a fake claude (a shell script that exits with a configured code after a configured delay), drive it via the IPC socket, assert state transitions.
- **Smoke:** run real claude under supervisor in a tmux pane, verify restart + inject + CWD invariant end-to-end. This is the "manual testing gate" from the release checklist.

## Files to Add / Modify

```
src/agent-doc/
  src/
    supervisor/
      mod.rs          # public entry: run(file, opts)
      pty.rs          # pty allocation + I/O forwarding threads (portable-pty)
      ipc.rs          # Unix socket accept loop + protocol
      state.rs        # crash classifier + ring buffer + state machine
      cwd.rs          # CWD resolution logic
      resize.rs       # ResizeWatcher trait + unix/windows impls
    start.rs          # thin wrapper: resolve opts, call supervisor::run
    main.rs           # unchanged CLI surface
  Cargo.toml          # + portable-pty, signal-hook (unix), windows-sys (windows)
  specs/
    supervisor.md     # this file
```
