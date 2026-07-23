---
name: supervisor
status: living
date: 2026-04-13
updated: 2026-07-22
---

# agent-doc Supervisor Spec

## Motivation

`agent-doc start` is a supervisor-lite: it wraps `claude` in a restart loop inside a tmux pane (see `src/start.rs:170`). It handles clean-exit prompts and non-zero auto-restart, but:

- **Has no pty** — `claude` inherits the tmux pane's tty directly. The supervisor cannot observe output, inject keystrokes, or detect prompt state.
- **CWD is inherited from the tmux pane** — whatever directory the pane happens to be in at spawn time, which is the root cause of the cross-project CWD drift bug this spec exists to fix.
- **Has no IPC** — external processes (editor plugins, `/agent-doc` routing from other panes) can only talk to the running claude via `tmux send-keys`, which is racy and cannot introspect state.
- **Crash recovery is a 2s sleep + `--continue`** — no escalation, no state inspection, no cooldown.

The supervisor graduates `start.rs` into a process that **owns** claude as a child behind a pty, holds a Unix-domain IPC socket per session, and enforces invariants (CWD, env, auto-restart cadence, external control) that a bare tmux-pane wrapper cannot.

## Implementation Status

| Submodule | Status | Notes |
|-----------|--------|-------|
| `supervisor/cwd.rs` | **landed** | Deterministic CWD resolution and source tagging are active in production. |
| `supervisor/pty.rs` | **landed** | `agent-doc start` now spawns the harness behind the supervisor-owned pty and forwards stdin/stdout through it. |
| `supervisor/screen.rs` | **landed** | The supervisor feeds filtered owned-PTY output into an `alacritty_terminal` screen model, so prompt/help/permission detection can inspect the current child viewport without relying on tmux `capture-pane` output. |
| `supervisor/resize.rs` | **landed** | Terminal resize events are forwarded to the child pty during the supervised session. |
| `supervisor/state.rs` | **landed** | Crash classification, restart cadence, waiting-input prompts, and halted-state handling are live. |
| `supervisor/ipc.rs` | **landed** | Per-session supervisor IPC serves `inject`, `restart`, `state`, `pid`, and `stop`. |
| `start.rs` wire-up | **landed** | The production `agent-doc start` path owns the supervisor lifecycle inline in `start.rs` while delegating focused pieces to `supervisor/*` modules. |
| Project controller registration | **landed** | Supervisor startup lazy-launches the project controller, registers the actor generation and supervisor lease, and reports lifecycle transitions through controller IPC. |
| `tmux-router` hybrid policy | **landed** | Control mode handles lifecycle/events, `pipe-pane` handles live output streams, and owned PTY input remains scoped to managed Claude/OpenCode supervisors that need byte-exact input. |

The original rollout plan in this document is retained for architectural context,
but the supervisor stack is now shipping. Current behavior should be read from
the "Implementation Status", "Actor state reporting", and the specs referenced
from the session-actor contract rather than from the old phased rollout notes.

## Non-Goals

- Not a tmux replacement. Supervisor still runs inside a tmux pane; tmux still owns the visual terminal the user sees.
- Not a sandbox. Supervisor does not restrict what claude can do — it just owns its lifecycle.
- Not a daemon for multiple sessions. **One supervisor = one claude = one session document.** Per-session daemons are easier to reason about than a global one.
- That boundary includes harness conversation identity. A replacement child must not use global "last conversation" selectors such as Claude `--continue`, Codex `resume --last`, or OpenCode `--continue`; those can attach an otherwise-correct pane/actor to another document's recent session. Without an exact durable harness-session binding, restart fresh and re-submit this supervisor's document trigger.
- Install/recycle is an ordered document handoff, not a kill-and-relaunch broadcast. The executable path is replaced by same-directory atomic rename so controller and supervisor exec calls always see a complete old or new binary. Background auto-install waits for a clean committed source tree, disables the nested `lib-install` recycle broadcast, marks every route-owned supervisor first, then marks controllers, exactly once. A supervisor with a live child adopts the existing PTY across `execve`; a supervisor without a live child starts one fresh document-bound replacement and auto-triggers its own file.
- Route-owned lifecycle is explicit at creation. Editor-origin Run Agent Doc and selected-document layout provisioning use `keep-alive`, so an idle prompt after commit or install remains the selected document's interactive owner. Controller/watchdog recovery uses `auto` and may reap a proven idle one-shot generation. Focus reconciliation must never recreate a keep-alive pane merely because a stale actor projection temporarily made it unfocusable.
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
5. Reports actor lifecycle facts to the project controller so the session actor
   owns authoritative state transitions.
6. Re-resolves the document frontmatter at quiet dispatch-ready prompt
   boundaries. When `agent:` now maps to a different harness and
   `agent_doc_agent_change_restart` is enabled, the supervisor logs
   `harness_change_detected`, gates with `agent_restart_boundary_gate`, requests
   a fresh restart, and records `agent_restart_performed` before spawning the
   new harness.

JetBrains "Restart Agent" menu invocations must also leave an
optverify-scannable `.agent-doc/logs/ops.log` marker before dispatching the
restart command:

```text
restart_agent_menu_invoked file=<relative-path> source=jetbrains action=restart_agent
```

That marker proves the menu path was actually invoked; the later
`agent_restart_performed` marker proves the supervisor boundary restart spawned
the fresh harness.

Route/startup code must not cold-replace a healthy live authoritative actor
solely because the document's `agent:` frontmatter changed. A live wrong-harness
actor is deferred to the boundary restart path; only an unhealthy supervisor or
a closed actor record is replaceable through the stale-authority path.

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
  2. Frontmatter `env:` map (already expanded by `agent_doc_config::env::expand_values`)
  3. `AGENT_DOC_SESSION=<uuid>` and `AGENT_DOC_DOCUMENT=<path>` (new, so claude can introspect its own session without parsing frontmatter)
- **No inheritance of `PWD` from the tmux pane** — supervisor sets `PWD` explicitly to match the resolved CWD, so shell-side `$PWD` matches the process-level CWD.

### Pty lifecycle
- Pty is allocated before claude spawns and destroyed after claude exits + IPC socket closes.
- SIGWINCH on the tmux pane → forwarded to the pty master so claude sees resize.
- `pty.rs` owns allocation and exposes the master reader/writer handles; it does
  not start an alternate stdin reader. On Unix, `io_threads.rs` owns the sole
  stdin→pty path and polls both raw stdin and a stop pipe, allowing child exit
  to join the forwarder before the supervisor reads a restart/quit prompt
  (`#af88`).
- Filtered child output is also fed into an `alacritty_terminal` screen model
  owned by the supervisor. Readiness, help-screen, and protected-prompt checks
  prefer that current viewport text and only fall back to the byte ring when the
  screen is empty, so cursor rewrites and line clears are interpreted as terminal
  state instead of append-only scrollback.
- On supervisor exit (user `q`), pty slave closes, claude gets SIGHUP.
- OpenCode permission prompts are a guarded stdin exception: when recent child
  output parses as the horizontal `Allow once` / `Allow always` / `Reject`
  permission dialog, the stdin forwarder rewrites legacy arrow-key escapes to
  Tab/BackTab selector keys before forwarding them to the child. Outside that
  active permission prompt, OpenCode receives the original arrow bytes so normal
  composer navigation is unchanged.
- Supervisor input diagnostics emit structured `tmux_input_event` lines at the
  input boundaries: operator stdin forwarding, IPC/auto-trigger injection, tmux
  pane submits, and active permission-prompt key translations. Each line
  includes source, destination, transform, key, byte count, and harness when
  known; prompt text is logged only as length plus SHA-256. Child-output
  Kitty keyboard-mode preserve/drop traces are verbose-only
  (`AGENT_DOC_TMUX_INPUT_DIAG` / `AGENT_DOC_DEBUG_STDIN`) so normal supervisor
  filtering cannot print diagnostics into the managed TUI while the operator is
  typing.
- Managed Claude/Codex/OpenCode supervisors redirect their own stderr to
  `.agent-doc/logs/supervisor-stderr.log` after startup. Routine idle-watch,
  stale-busy reconcile, and stale-binary hot-reload diagnostics must stay in the
  session log / ops log instead of painting over the foreground TUI after
  `/clear`, restart, or inter-queue-item recycle.
- Codex background-terminal banners are active-turn evidence. If recent pane
  output contains `Waiting for background terminal (... esc to interrupt)`, the
  supervisor idle-queue watch and route readiness checks must treat the pane as
  busy even when Codex also renders an input prompt; text typed into that prompt
  would be queued, not dispatched.
- Queue-dispatch progress follows the same foreground-safety contract. Dispatch
  attempts record redacted `queue_dispatch_progress` / `queue_dispatch_warning`
  events in `.agent-doc/logs/ops.log` with command byte counts and SHA-256, but
  they must not mirror raw command text or progress lines into the foreground
  OpenCode TUI unless verbose input diagnostics are explicitly enabled.

### Actor lifecycle reporting

- Startup calls the project controller before the child launches:
  `start_session` records the actor generation in `starting`, and
  `register_supervisor` records the supervisor pid/socket lease.
- Prompt observation reports `ready` with reason `prompt_ready`. This happens on
  the first prompt for a child and after later `busy` dispatches return to an
  idle prompt.
- Supervisor-owned dispatch paths report `busy` before injection, using
  `ipc_inject` for routed IPC and `auto_trigger_inject` for restart-triggered
  reopen commands.
- Clean exits that require operator input report `waiting_input`; flap halts
  report `blocked`; supervisor shutdown reports `closed`.
- Every lifecycle report carries the session id, pane id, and generation. The
  controller rejects stale reports instead of letting an old supervisor mutate a
  newer actor owner.

## IPC Socket

Per-session Unix-domain socket at `.agent-doc/supervisor/<session-uuid>.sock`.

Protocol: length-prefixed JSON (same frame format as `ipc_socket.rs`, so the existing client code in FFI can be reused).

### Methods

| Method | Request | Response | Notes |
|--------|---------|----------|-------|
| `restart` | `{ "mode": "fresh" \| "continue" }` | `{ "ok": true, "pid": <u32> }` | Kills current claude, relaunches |
| `inject` | `{ "bytes": "<base64>" }` | `{ "ok": true, "n": <usize> }` | Write bytes to pty master |
| `state` | — | `{ "running": bool, "pid": u32?, "restart_count": u32, "state": "...", "actor_state": "..."?, "current_harness": "..." }` | Includes supervisor health, the current actor lifecycle state, and the live child harness identity |
| `pid` | — | `{ "pid": u32? }` | Convenience shortcut |
| `stop` | `{ "graceful": bool }` | `{ "ok": true }` | Shuts down supervisor + child |

Socket is created with mode `0600`. Opaque to anything except the FFI library, which exposes typed C ABI wrappers.

### External control use cases

1. **`/agent-doc` routing from a different tmux pane:** instead of `tmux send-keys`, the route subcommand opens the supervisor socket and calls `inject` with `/agent-doc <file>\r`. Removes the 5s sleep hack + race conditions in `start.rs:229`.
2. **Editor plugin "restart claude" button:** IntelliJ plugin opens the socket, calls `restart`, displays the returned pid.
3. **Crash-state introspection for health dashboards:** `state` returns restart count plus supervisor/actor state so a cleanup hook can escalate (e.g., "5 restarts in 60s → stop and notify").

### Actor state reporting

The supervisor is responsible for keeping the authoritative session-actor store
in sync with the live child lifecycle without creating a new ownership
generation:

- child launch after the initial `session_start` moves the actor to `busy`
- idle prompt visibility moves it to `ready`
- clean-exit or resume-failure prompts move it to `waiting_input`
- halted restart policy moves it to `blocked`
- final supervisor exit moves it to `closed`
- IPC or auto-trigger dispatch writes also mark the actor `busy` before bytes
  are injected into the child pty
- the IPC `state.current_harness` projection changes immediately when an
  in-loop frontmatter `agent:` handoff spawns the replacement child
- route accepts a live harness mismatch as a boundary handoff and sends no
  prompt bytes to the old child; idle watch preserves an active turn, switches
  at the first safe idle boundary, and auto-triggers the document on the fresh
  child exactly once
- queue pause holds that accepted handoff until resume, while disabled
  `agent_change_restart` remains an explicit rejection rather than an implicit
  manual-restart recovery path

Those updates must fail closed when the supervising pane/session no longer owns
the authoritative generation.

Both `route` and `resync` also use the supervisor socket as part of registered-document ownership proof. The primary top-down check requires the controller's canonical document binding to agree with the live pane, recorded supervisor PID, and reported `supervisor_instance_id`. When tmux argv/path inspection no longer proves ownership, the socket's `Pid` method remains a secondary diagnostic that maps the live supervisor PID back to the tmux pane before treating the transactional binding as stale.

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
                         EXCEPT when a fresh/fresh-restart Codex child exits
                         cleanly before it ever surfaces an idle prompt; treat
                         that as failed startup provenance and restart fresh
                         instead of chaining `--continue`
                         EXCEPT when stdin EOF (Ctrl+D) detected → prompt user
                         (Enter to restart fresh / q to exit) so the operator
                         can intentionally quit the supervisor cleanly
                         EXCEPT when stdin-forwarded Ctrl+C terminates the
                         child → prompt user with that same menu instead of
                         treating the exit like a transient crash
                         AND only stdin-forwarded Ctrl+C counts for that path;
                         route/plugin-injected interrupts that bypass the
                         stdin writer stay on the automatic recovery path
                         EXCEPT when a fresh/fresh-restart Codex child exits
                         before it ever surfaces an idle prompt and no
                         forwarded operator quit key was observed: treat that
                         clean exit as failed startup provenance and restart
                         fresh automatically instead of prompting
                         AND the supervisor must log the prompt outcome
                         (`user_quit*`, `user_restart_fresh`, invalid input) so
                         later `session_start` / `session_end` transitions keep
                         user-input provenance in the session log
                         AND those supervisor prompts must switch stdin into a
                         canonical local prompt mode instead of trusting the
                         inherited parent harness tty flags, so Enter keeps
                         working even when the outer binding session is raw-ish
                         AND stdin EOF at the restored Ctrl+D prompt counts as
                         quit, because that prompt now only appears after a
                         real visible child prompt and reflects an intentional
                         operator quit/restart choice again
                         AND stdin EOF at the remaining resume-failure prompt
                         counts as `restart fresh`, not as quit, so detached
                         stdin cannot close the pane during keepalive recovery
                         AND non-empty non-`q` input is rejected with a
                         re-prompt instead of silently restarting fresh
AND resume auto-trigger primarily accepts a prompt line that
appears in the current resumed child's filtered pty output;
when the child-owned terminal renderer misses a prompt that the
owned tmux pane can see, it may fall back only after the current
child has emitted output, the actor independently reconciles to
ready, and the pane remains dispatch-ready for two consecutive
polls; stale tmux history alone is never enough
AND when the replacement child never re-establishes a prompt
(`auto_trigger_timeout` / trigger delivery failure), treat the
handoff as failed provenance. The watcher fails closed after 60s,
records `startup_miss`, and never types into an unproven child;
a later explicit route may recover once the owned pane becomes ready
Transient: sleep 2s, restart fresh and document-bound, state Healthy
Flapping:  sleep 30s, restart fresh and document-bound, state Degraded
                   on 5th consecutive failure → state Halted
        Halted:   do not restart. Print "supervisor halted — run 'agent-doc
                  supervisor resume <session>' to retry"
```

State is surfaced via `state` IPC method so dashboards / cleanup hooks can observe it.

## `start.rs` integration

`agent-doc start` now always runs through the supervisor-owned start path.

- The production code keeps the orchestration loop in `start.rs` instead of
  collapsing everything into one `supervisor::run` wrapper.
- `supervisor/*` owns the bounded helper concerns: pty management, IPC, resize,
  environment/CWD resolution, and crash-policy primitives.
- Release behavior is already live for sessions spawned through the current
  binary; existing pre-supervisor panes are unaffected until they are restarted.

## Five Hard Parts — Answers

From the previous exchange:

1. **Pty vs. raw inheritance** — pty. Use `portable-pty` crate for cross-platform (we need at least Linux + macOS for dev + manylinux for PyPI builds). The pty allows stdin/stdout forwarding while giving us an injection point.

2. **CWD determinism** — resolved at supervisor startup from CLI flag > frontmatter > project root > doc parent. Set both on the child process and inside the pty via a `cd` command before handing control to claude.

3. **Crash recovery** — state machine with ring buffer (Healthy → Transient → Flapping → Halted). Flap detection via `exits_in_last_60s`.

4. **External control** — per-session Unix socket, length-prefixed JSON, four methods (restart, inject, state, stop). Reuses frame format from `ipc_socket.rs`.

5. **IPC lifecycle** — supervisor owns the socket for its lifetime. Socket file is cleaned up on normal exit and on `stop`. Stale sockets are detected by connecting during `register()` — if connect fails with ECONNREFUSED and the supervisor PID in controller state is dead, delete the stale socket.

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

Single log file per session at `.agent-doc/logs/<session-uuid>.log`, same path as today. Supervisor events use a `[supervisor]` tag prefix on each line for filtering. Format: `[<timestamp>] [supervisor] <event> key=value ...`, where `<timestamp>` is a human-readable ISO-8601 UTC stamp `YYYY-MM-DDTHH:MM:SSZ` (`#opslogts`) so operators reading the log can correlate events to wall-clock time. The staleness / accretion / startup-miss / `gate_verify` scanners read it back through `agent_doc_log_time::parse_log_timestamp`, which also still accepts the pre-`#opslogts` bare-epoch form for backward compatibility.

Example:
```
[1713041234] session_start file=tasks/plan.md pane=%12 session=abc12345
[1713041235] document_cycle phase=preflight_started cycle=cycle-1713041235000 event=preflight_started
[1713041237] document_cycle phase=response_captured cycle=cycle-1713041235000 event=response_captured capture_id=cycle-1713041235000
[1713041238] document_cycle phase=write_applied cycle=cycle-1713041235000 event=write_template
[1713041239] document_cycle phase=committed cycle=cycle-1713041235000 event=commit_success
[1713041234] [supervisor] pty_allocated rows=40 cols=120
[1713041234] [supervisor] cwd_resolved path=/home/brian/work/agent-loop source=project_root
[1713041234] [supervisor] claude_spawn pid=54321 mode=fresh
[1713041290] [supervisor] claude_exit code=0 exit_kind=success exit_status="Success"
[1713041291] [supervisor] user_action=restart
[1713041291] [supervisor] claude_spawn pid=54398 mode=continue
[1713041390] [supervisor] codex_exit code=129 exit_kind=signal exit_signal="Hangup" exit_status="Terminated by Hangup"
[1713041390] [supervisor] restart_eval pane=%12 harness=codex exit_code=129 exit_kind=signal exit_signal="Hangup" exit_status="Terminated by Hangup" auto_trigger_outcome=sent ctrl_d=false state=healthy action=restart_after
[1713041390] [supervisor] auto_restart delay=2s with_continue=true restart_count=1
[1713041391] [supervisor] codex_spawn pid=54444 mode=continue
[1713041421] [supervisor] auto_trigger_timeout pane=%12 harness=codex reason=no_prompt_after_60s
[1713041425] [supervisor] resume_restart_failed pane=%12 harness=codex outcome=timeout recent_failures=1 window_secs=900 restart_count=1
[1713041450] [supervisor] supervisor_exit reason=user_quit_clean_exit pane=%12 restart_count=1
[1713041450] session_end
```

Document closeout phases share the same per-session log:
- `document_cycle phase=preflight_started ...`
- `document_cycle phase=response_captured ...`
- `document_cycle phase=write_applied ...`
- `document_cycle phase=committed ...`

Those entries are not supervisor lifecycle events, but they must land in the same `.agent-doc/logs/<session>.log` timeline so crash forensics can line up child exit / pane-loss provenance with the exact `state.db` closeout transition.

At minimum, harness exit lines must preserve:
- exit code
- exit kind (`success`, `exit_code`, or `signal`)
- signal name when the child died from a signal
- the rendered child status text used for operator forensics

The final supervisor-owned closeout must append `supervisor_exit reason=...` before `session_end` so later crash analysis can distinguish deliberate quits, IPC stops, and flapping halts from missing-pane recovery written by other components.

Session-log consumers must treat any event whose first token is `session_end` as a closeout boundary, even when origin metadata follows on the same line (for example `session_end origin=registry_rebind ...` or `session_end origin=sync_missing_pane`). Provenance analysis must not require the whole line to equal the bare literal `session_end`.

When pane-loss recovery discovers that the current cycle is already `response_captured` or `write_applied`, the same session log must also record the recovery attempt before the synthetic `session_end origin=sync_missing_pane` closeout. The minimum provenance is:
- `sync_missing_pane_closeout_recovery_start ... phase=response_captured|write_applied durable_capture=<bool>`
- either `sync_missing_pane_closeout_recovery_result ... outcome=...` or `sync_missing_pane_closeout_recovery_failed ... reason=...`

That keeps pane-loss forensics and closeout forensics on the same timeline: operators can tell whether sync replayed the durable capture, finished a missing commit boundary, or failed closed and left the capture for later `preflight` / `repair`.

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

### Idle-queue watch (`#jb-run-agent-doc-busy-queue-dispatch-deadlock`)

When a busy-pane `Run Agent Doc` route cannot inject into an active turn, it
inserts the prompt **ahead of queued active-loop items** in plain `agent:queue` (a
manual operator dispatch preempts the loop rather than landing at the tail —
`#jb-run-preempt-autoloop-priority`; the priority insert lands after any leading
queue directive such as a preset/start fence and never supersedes a lone active
prompt, so the pending loop item is preserved; route-owned queue writes never add
`auto` and strip legacy `auto` from touched tags), sets `queue_active: true`,
and returns `Ok` (`route.rs` `AuthoritativeActorDispatchAction::DispatchOnlyBusyQueue`). The
drain is otherwise harness-delegated: the Codex `Stop` hook drains on turn-end,
and Claude relies on `/loop` or a manual re-trigger. A Claude session not running
`/loop` therefore has no guaranteed drain, so the queued head can sit forever —
the operator-perceived "deadlock" that forces an agent restart.

The supervisor closes that gap with a long-lived **idle-queue watch** thread,
distinct from the one-shot restart auto-trigger:

- It runs for the whole child lifetime (spawned next to the auto-trigger,
  cancelled + joined on child exit), polling on the same
  `AUTO_TRIGGER_POLL_INTERVAL`.
- The drainable head is the shared `queue_continuation::live_continuation_head`
  definition: frontmatter `queue_active: true`, explicit `go` mode
  (`queue: go` or marker-side `go`), an active `resolve_activation`, and a ready
  prompt head. Inactive-residue queues (`queue_active: false`) and plain
  persisted-active queues without `go` are never drained passively. An explicit
  dispatch-only `Run Agent Doc` against a busy authoritative actor may promote an
  already-startable inactive queue (marker-side `go`/`start`, a start fence with
  a ready prompt head, or legacy `auto`) by setting `queue_active: true`, syncing
  the snapshot, stripping legacy `auto`, and returning the same deferred
  busy-route feedback; unattended continuation after closeout still requires
  explicit `go`. Plain inactive queues without a start trigger stay inert.
- The drain decision is the pure, deterministically tested
  `idle_queue_drain_decision(prompt_visible, active_head, last_dispatched)`:
  - `Dispatch` only on a busy→idle transition (`prompt_visible`) with an active
    head that differs from the last one dispatched.
  - `SkipNotIdle` whenever the pane is mid-turn — the same
    no-inject-into-active-turn invariant the route busy path enforces.
  - `SkipAlreadyDispatched` dedups a head that is still present after a dispatch
    (cycle not yet consumed, or the dispatch failed to drain), so a stuck head
    cannot hot-loop the watch every idle tick.
- If actor state, supervisor runtime actor state, and controller lease are all
  `ready` while a stale Codex queued-draft renderer cue still makes the pane
  probe look `alive-busy`, the watch debounces that ready/busy conflict for the
  stale-busy repair window, logs `owned_pane_ready_busy_conflict`, and treats the
  pane as dispatchable. Active-turn, permission, hook-review, shell-search,
  help, and clean-exit blockers still produce `SkipNotIdle`.
  - `SkipNoActiveHead` clears the dedup so a later re-enqueue of the same prompt
    text fires again.
- Background context reset injection is disabled for queue heads: `[clean-session]`,
  `[focused-cycle]`, and accretion/threshold reset reasons drain in pane instead
  of causing the supervisor to submit `/clear` or `/new`. Only explicit operator
  clears and explicit queued slash-command heads may create a context-clear
  projection. Busy-loop operator clears first record `QueueContextClearDeferred`;
  the idle-watch promotes that projection to `QueueContextClearStarted` only
  after it submits the deferred clear. For explicit in-flight sources, the
  projection survives supervisor recycle/restart so the replacement watcher
  blocks drains until the clear settles, or presses the shared submit key once
  when the clear command is still visible in the composer. Legacy or
  background-sourced projections are settled rather than submitted. Once the pane
  settles for `CLEAR_COOLDOWN_RESUME_IDLE_TICKS` consecutive polls the projection
  records `QueueContextClearSettled`. A
  manual clear cooldown remains authoritative for a plain operator clear with no
  active queue (it suppresses passive dispatch until cleared by the existing
  operator route path) and for an operator-deferred clear that explicitly paused
  the loop. But a manual clear cooldown must NOT suppress an active go-mode queue
  drain forever (`#clearcontresume`): the cooldown only exists to avoid
  dispatching a trigger into an in-flight `/clear`, so once the cleared pane
  settles to a fresh idle prompt for `CLEAR_COOLDOWN_RESUME_IDLE_TICKS`
  consecutive polls AND a `queue_active: true`
  head is waiting AND no operator-deferred clear is still pending delivery, the
  watch settles the cooldown projection and resumes the drain. The recycle +
  clear is then a continuation *step*, not a stall. The decision is the pure,
  unit-tested `decisions::clear_cooldown_resume_ready`. The full operator recycle
  + clear pipeline — `admin recycle` (mark recycle at the next idle boundary) →
  in-place `execve` recycle preserving the live pane → `session clear` (record
  the clear-cooldown/deferred-clear projection) → submit/settle → cooldown
  auto-expiry → go-mode drain resume — is also driven end-to-end by the
  deterministic SimWorld engine
  (`SimCommand::SupervisorIdleQueueTick`), which calls the SAME production
  predicates (`supervisor_recycle_action`, `clear_cooldown_resume_ready`,
  `idle_queue_drain_decision`) the live idle-queue watch uses rather than
  reimplementing the policy, so the recycle + clear pipeline is simulated offline
  across its interoperating systems.
- `#wd40` state-flush: an explicit `admin recycle` recycles at the next turn
  boundary even when the installed binary already matches the running supervisor
  (`supervisor:fresh`). The recycle then restarts the supervisor *process*, which
  rebuilds its in-memory state — flushing a lagging CRDT projection
  (`projection_lag`) that can keep re-pinning a queue head (`#rt83` phantom-pin
  churn) even though there is no stale binary to swap. `supervisor_recycle_action`
  therefore returns `RecycleImmediate` for `explicit_admin` on a fresh binary
  (previously a silent no-op that left the operator unable to clear the lagging
  projection); a bare wedge on a fresh binary still stays a no-op. When a
  self-driving `/loop` holds the drain `turn_active`, the idle-watch records a
  `state_flush_drain` recycle-yield reason in the lazily-backed supervisor
  recycle projection (mirroring the `stale_binary_drain` path) so the loop
  yields one inter-item boundary and the process restart fires. Preflight,
  session-check, and queue continuation detection observe the projection through
  controller RPC instead of a `.agent-doc/recycle-yield` marker. It still respects
  `turn_boundary` — a fresh-binary flush never drops a live turn.
- `#recycleforce` operator override: `agent-doc admin recycle --force` is a real
  flag (no `-- ` separator needed) and composes with `--all-projects` and the
  single-project form. `--force` overrides the controller-side in-flight-dispatch
  deferral: the `recycle_force` RPC sets a `recycle_forced` flag so
  `controller_recycle_idle` returns true without the open-dispatch idle probe
  (pure decision `force_overrides_in_flight_gate`, which still requires the
  controller to be `Stable` so a forced recycle never strands a half-promoted
  handoff replacement). The recycle still fires at the next serve-loop tick, never
  mid-RPC, but it MAY interrupt an in-flight turn — that is the point of `--force`.
  When NO live controller answers the single-project form and a document path was
  supplied (`agent-doc admin recycle <FILE> --force`), the no-op "nothing to
  recycle" message is replaced by an escalation to the kill+cold-start path
  (`session restart-supervisor`, which carries the same self-ancestor guard as
  `admin kill-supervisor`, so a forced escalation never tears down the caller's own
  ancestor supervisor). The escalation gate is the pure
  `recycle_force_should_escalate_dead_supervisor` decision (requires force, a no-op
  recycle, and a non-directory document target). Without `--force`, behavior is
  byte-for-byte the prior defer-at-idle-boundary recycle. `--json` adds a
  `forced: bool` field (and `escalated_cold_start: bool` on the single-project arm).
- On `Dispatch` it injects a harness-specific trigger through the same
  `auto_trigger_inject_command` path (capability-proof gated, actor marked
  `busy` before bytes). Claude/OpenCode receive the normal harness trigger
  (`/agent-doc <FILE>`), while Codex receives the bare Codex-compatible trigger
  (`agent-doc <FILE>`). The queue-continuation prompt contract belongs to the
  installed harness instruction surface and runbooks, not to the editor/supervisor
  payload itself. A failed inject is not recorded as dispatched, so it retries on
  the next idle tick. Successful drains log `idle_queue_watch_drain` with
  `payload_kind=trigger` and
  `submit_mode=tmux_hex_text_cr|pty_cr`; failures log
  `idle_queue_watch_drain_failed`.
- **Stale-busy self-heal (`#stale-busy-after-auto-inject-no-clear`).** The
  one-shot busy→ready completion transition on the pty→stdout thread is
  edge-triggered on the latest output chunk. When an injected turn returns but
  its composer redraw lands split so the final chunk carries no detectable
  prompt, the actor can stay wedged `busy` over an idle pane with no further
  bytes to retrigger ready, so the session presents as "truly stuck" and even a
  pane kill + restart re-enters the state. The watch closes this with
  a polling backstop driven by the pure
  `stale_busy_idle_reconcile_decision(actor_busy, pane_has_busy_cue, clear_cooldown_active, ticks)`:
  when the in-memory actor is `busy`/`starting`, a fresh `tmux capture-pane`
  shows no harness `has_busy_cue` (the same direct evidence `route.rs` uses for
  its stale-busy repairs — not the supervisor's edge-triggered pty buffer that
  missed the redraw), no clear cooldown is pausing the loop, and the
  idle-over-busy condition has held for `STALE_BUSY_RECONCILE_TICKS` consecutive
  polls (~2s debounce so a turn still spinning up is never cut short), the watch
  transitions the actor back to `ready` (`caller=supervisor reason=idle_pane_reconcile`,
  persisted through `mark_lifecycle`), resets the prompt latch, and logs
  `idle_queue_watch_stale_busy_reconciled`. It must preserve the dispatch dedup
  for the current head: if the injected command returned without consuming the
  same active head, the next idle tick must skip it as `SkipAlreadyDispatched`
  instead of re-injecting the same drain payload in a loop. The dedup clears
  only when there is no active head or the head advances. This recovers the
  wedge with no pane kill and no operator `session status`/`session clear`.

Live end-to-end verification (a real busy Codex/Claude pane returning to idle and
draining the route-appended head with no duplicate injection into the active
turn) stays operator-gated.

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
      pty.rs          # pty allocation + reader/writer handles (portable-pty)
      io_threads.rs   # interruptible stdin forwarding + pty output forwarding
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
