# Plan — agent-doc in a Coder workspace (JetBrains Toolbox + VS Code, IDE-hosted tmux)

Status: **not started** — planning only. Backlog phases `#coder1`..`#coder7`.

## Goal

Make agent-doc usable inside a [Coder](https://coder.com/) workspace, where the
operator has **no external terminal emulator and no separate shell** — the only
shell surface is the IDE's terminal tool window running on the remote workspace
(JetBrains remote backend via Toolbox/Gateway, or VS Code Remote-SSH /
code-server).

## What already works (verified in this repo)

This is the important part of the diagnosis: **the IPC/CRDT plane does not need
any new transport.**

- In both Coder IDE modes the editor process that hosts the agent-doc plugin
  runs **on the workspace**, not on the operator's laptop:
  - JetBrains Toolbox/Gateway starts a remote IDE *backend* on the workspace; the
    local Toolbox window is a thin client.
  - VS Code Remote-SSH / code-server runs the *extension host* on the workspace.
- agent-doc's editor endpoints are unix sockets under
  `<project_root>/.agent-doc/agent-doc-<pid>.sock`
  (`agent-doc-ipc-io/src/lib.rs:165`, `socket_path_for_pid`). Plugin and binary
  are same-host, same-user, same filesystem → the existing endpoint fan-out,
  CRDT replica forwarding, and reliable-sync registrations work unchanged.
- `tmux_router::Tmux::new_session` (`src/tmux-router/src/tmux.rs:422`) already
  creates a **detached** session (`tmux new-session -d`). The binary can bring up
  a tmux server with no attached client and no TTY of its own.
- Pane creation inside an existing server is already owned by the controller
  (`editors/SPEC.md:25` — manual sync "must create and register one in the
  resolved target session").

## What actually breaks

1. **`agent-doc start` refuses to run outside tmux.**
   `agent-doc-start-io/src/lib.rs:934` (`ensure_inside_tmux`) hard-bails with
   *"not running inside tmux — start a tmux session first: tmux new-session -s dev"*.
   In Coder the operator has nowhere to type that except the IDE terminal, and
   nothing tells them so.
2. **`agent-doc terminal` spawns a GUI terminal emulator.**
   `src/terminal.rs` resolves `[terminal] command` (e.g.
   `wezterm start -- {tmux_command}`) or `$TERMINAL`, then spawns it detached. A
   Coder workspace container has no `$DISPLAY` and no terminal emulator
   installed, so the one bootstrap affordance for plugin users is dead there.
3. **Neither plugin can open a terminal.** Confirmed by grep: the JetBrains
   plugin never touches `TerminalToolWindowManager` / `ShellTerminalWidget`, and
   the VS Code extension never calls `window.createTerminal` / `sendText`. There
   is no IDE-hosted shell path at all today.
4. **Workspace stop/start destroys the tmux server** but leaves
   `.agent-doc/` registry rows and `*.sock` files on the persistent volume.
   Coder workspaces are stopped aggressively (idle timeout, nightly autostop), so
   this is the *normal* case there, not an edge case.

## Session→project association (already exists — landed fix 2026-08-23)

The binding does **not** need a new command. It already exists and is
project-scoped:

- `agent-doc session` — show the configured session (or `(auto-detect)`)
- `agent-doc session set <name>` — pin it; also migrates the `agent-doc` +
  `stash` windows via `tmux move-window` and closes a superseded orphan session
- `agent-doc session clear` — return to auto-detect

It persists as `tmux_session` in `<project_root>/.agent-doc/config.toml`
(`agent-doc-project-config-io`: `project_tmux_session` / `update_project_tmux_session`
/ `clear_project_tmux_session`), and `start` self-heals it —
`rebind_project_tmux_session_if_expected_dead` (`agent-doc-start-io/src/lib.rs:1130`)
rewrites the binding when the configured session is dead.

**Local vs remote needs no extra scoping.** `.agent-doc/` is gitignored, so a
laptop checkout and a Coder workspace checkout each hold their own binding.
tmux runs on the cloud instance; the workspace's `config.toml` is what governs
it there.

**The gap was one path, now fixed** (`src/terminal.rs`, commit `35efb28`):
`resolve_session_name` ignored the project binding and hardcoded `"0"` whenever
no live pane existed — exactly the cold-start case an IDE-hosted workspace
depends on, where no tmux server is running and the registry scan cannot derive
a session. Precedence is now explicit `--session` flag > project `tmux_session`
> `"0"`; frontmatter stays deprecated and unconsulted. A live registry pane
still outranks all of it (`resolve_target_session` scans first).

So Phase 2/5 below **wire into** `agent-doc session set`; they do not introduce a
parallel binding.

## Design decisions (answers to the open questions)

### Auto-start tmux, or require the operator to start it?

**Auto-start by default; keep an explicit opt-out that shows the dialog.**

The binary already has the primitive (`Tmux::new_session`, detached, idempotent
via `has-session`), and the registry already dedupes live panes, so auto-start
costs nothing and is re-entrant. Requiring a manual `tmux new-session` on every
workspace start is friction that will silently strand routes — and a Coder
workspace restarts far more often than a laptop.

The dialog message is still needed, for three cases that auto-start cannot fix:

- `tmux` is not installed in the workspace image (and the operator may have no
  sudo) → message names the fix as an image/`coder_script` change, not
  `apt install`.
- `[terminal] auto_start_tmux = false` (explicit operator opt-out).
- Auto-start succeeded but no client can attach (no IDE terminal API available).

### If we integrate with the IDE's *remote* tab, do we also support the IDE's terminal?

**They are the same surface — there is no second thing to build.** The
"remote tab" is the IDE terminal tool window; because the JB backend / VS Code
extension host runs on the workspace, `createTerminal` already lands on the
workspace shell. `vscode.env.remoteName` and the JB remote-backend flag change
*what we report in diagnostics*, not which API we call. So supporting the IDE
terminal is not extra scope; it is the whole feature.

### Should we support running tmux via the IDE locally, or wait?

**Ship it in the same change — but never let it pre-empt an existing external
session.** The plugin terminal API cannot tell local from remote, so excluding
local would mean adding a guard, i.e. more work for less capability.

The safety property that protects the current "I run agent-doc from a separate
shell" workflow is resolution order, not a local/remote switch:

1. A live session for this project in the durable registry **always wins** —
   attach to it, never create a second one. (`src/terminal.rs`
   `find_active_project_session` already does this; keep it as the first rule.)
2. Only when no live session exists does the host selection below run.

So locally, the external shell keeps ownership exactly as today; the IDE path
only fires on a cold start with no session — which is precisely the Coder case.

### How do we configure external terminal vs IDE terminal?

Extend the existing `[terminal]` table (`agent-doc-config/src/lib.rs:114`)
rather than inventing a second config surface:

```toml
[terminal]
# auto | external | ide | none
host = "auto"
# external host only; existing key, unchanged semantics
command = "wezterm start -- {tmux_command}"
# false => never create a session, show the operator dialog instead
auto_start_tmux = true
# optional explicit session name; otherwise registry scan then "0"
session = "agent-doc"
# what the IDE tab runs once open; {session} is substituted
attach_command = "tmux new-session -A -s {session}"
```

`host = "auto"` resolution:

| order | condition | host |
|---|---|---|
| 1 | live registry session for this project, client attached | none (no-op) |
| 2 | live registry session, detached | current host, attach only |
| 3 | IDE-hosted environment detected **and** a plugin endpoint is registered | `ide` |
| 4 | `[terminal] command` or `$TERMINAL` resolves | `external` |
| 5 | otherwise | dialog / stderr instruction |

Per-document override goes in frontmatter as `terminal_host:` so a single
workspace can mix an IDE-hosted document with an external-shell one.

## Phase 1 — environment capability detection (`#coder1`)

### Objective

Classify the terminal-host environment once, in the binary, so plugins and CLI
share one answer instead of each guessing.

### Changes

- New `agent-doc-terminal-host` (or a module in `agent-doc-config`) exposing
  `TerminalHostEnv { coder: bool, ide_remote: Option<IdeRemoteKind>, display: bool, tmux_installed: bool, tmux_server_running: bool }`.
- Signals: `CODER_*` env prefix (`CODER_AGENT_URL` / `CODER_AGENT_TOKEN` /
  `CODER_WORKSPACE_NAME`), absence of `DISPLAY`/`WAYLAND_DISPLAY`, presence of
  `SSH_CONNECTION`, `which tmux`, `tmux has-session`.
- **Verify the exact Coder env var names against a live workspace before
  landing** — do not ship on assumption. Same for the JB remote-backend probe
  (`idea.is.remote.dev` system property vs a platform API) and
  `vscode.env.remoteName` values.
- `agent-doc env --json` diagnostic subcommand printing the classification plus
  the resolved terminal host and why.

### Verification

- Unit tests over a fake env map for: laptop+DISPLAY, Coder container, plain SSH
  box, container with tmux missing.
- `agent-doc env --json` in this repo reports `external` and explains it.

## Phase 2 — headless tmux bootstrap (`#coder2`)

### Objective

Let agent-doc bring up its own tmux server without a TTY, and replace the
dead-end `ensure_inside_tmux` bail with a resolvable path.

### Changes

- `agent-doc tmux ensure <FILE> [--session <name>]`: idempotent — `has-session`
  → else `Tmux::new_session` detached; prints the resolved session name and the
  attach command on stdout, JSON with `--json`.
- Rewrite `ensure_inside_tmux` (`agent-doc-start-io/src/lib.rs:934`): when not
  inside tmux, consult the Phase 1 classification. If auto-start is permitted,
  ensure the session and **re-exec the start into that session's pane** instead
  of bailing. If not permitted, keep failing closed but with the host-specific
  message (image change for a container, not `apt install`).
- Never create a second session when the registry already holds a live one.

### Verification

- `agent-doc tmux ensure` twice in a row → one session, second call is a no-op.
- `agent-doc start` from a non-tmux shell with auto-start on → session created,
  pane registered, route dispatched.
- Auto-start off → fails closed, message names the operator action.

## Phase 3 — JetBrains IDE-hosted terminal (`#coder3`)

### Objective

Open a terminal tab in the JB Terminal tool window (local or remote backend) and
attach it to the agent-doc tmux session.

### Changes

- New `IdeTerminalHost` in the JB plugin using
  `org.jetbrains.plugins.terminal.TerminalToolWindowManager` to create a tab
  named `agent-doc` and run the resolved `attach_command`.
- Reuse: if a tab named `agent-doc` already exists and its shell is alive, focus
  it rather than opening another.
- Wire into the existing bootstrap path so a Run/Sync action with no live pane
  calls `agent-doc tmux ensure` then attaches the tab, instead of surfacing the
  "no pane" failure.
- Fallback dialog (`Messages`/notification) when the terminal API is
  unavailable or auto-start is disabled: shows the exact attach command with a
  **Copy** action.
- Declare the `org.jetbrains.plugins.terminal` dependency in `plugin.xml` and
  handle its absence gracefully (some JB IDEs/configurations ship without it).

### Verification

- Local IDEA with no tmux session → tab opens, session created, pane registers,
  Run dispatches.
- Existing external session → **no** tab is opened (rule 1 above holds).
- Terminal plugin disabled → dialog path, no exception.

## Phase 4 — VS Code IDE-hosted terminal (`#coder4`)

### Objective

Parity with Phase 3 through the VS Code terminal API.

### Changes

- `window.createTerminal({ name: 'agent-doc', cwd: projectRoot })` +
  `sendText(attachCommand)`; reuse an existing `agent-doc` terminal from
  `window.terminals` when its `exitStatus` is undefined.
- Same bootstrap wiring and the same fallback via
  `window.showWarningMessage` with a **Copy command** action.
- Report `vscode.env.remoteName` into session diagnostics so a wedge in a
  remote workspace is distinguishable from a local one.
- Update `editors/plugin-parity.tsv` with the new capability row.

### Verification

- code-server / Remote-SSH workspace with no tmux → terminal opens on the
  workspace, session created, route dispatches.
- Existing session → reuse, no second terminal.

## Phase 5 — config + frontmatter surface and docs (`#coder5`)

### Objective

Make host selection explicit, documented, and overridable per document.

### Changes

- Extend `TerminalConfig` (`agent-doc-config/src/lib.rs:114`) with `host`,
  `auto_start_tmux`, `session`, `attach_command`; keep `command` semantics
  unchanged for backward compatibility.
- `terminal_host:` frontmatter override, resolved through the normal
  frontmatter → project config → global config precedence.
- Implement the resolution table above as a pure function with tests, consumed
  by `src/terminal.rs`, the start path, and both plugins (via the CLI/JSON, not
  re-implemented per plugin).
- Docs: a `runbooks/coder-workspace.md` covering workspace image requirements
  (tmux in the image or a `coder_script`), the two IDE paths, and the
  external-shell-still-wins rule. Cross-link from README and SPEC.

### Verification

- Precedence tests for every layer.
- `host = "external"` inside a Coder workspace fails closed with a clear reason
  rather than silently doing nothing.

## Phase 6 — workspace restart resilience (`#coder6`)

### Objective

Survive Coder workspace stop/start, which kills the tmux server while
`.agent-doc/` state persists on the volume.

### Changes

- Record the tmux server identity (server PID + start time, or
  `#{pid}`/`#{start_time}` from `display-message`) alongside registry rows.
- On the first command after a mismatch, treat every pane row from the previous
  server as dead: GC them and the orphaned `agent-doc-<pid>.sock` files in one
  pass, then re-bootstrap.
- Confirm existing `agent-doc-gc-io` covers whole-server loss; extend rather
  than duplicate if it partially does.
- Guard: refuse to create sockets when `<project_root>/.agent-doc` is on a
  filesystem that cannot host `AF_UNIX` (some network/9p mounts), with a clear
  error naming an override dir instead of a confusing bind failure.

### Verification

- SimWorld test: registry rows from server A, server B boots, first command
  reconciles to zero stale rows and one live session.
- Stale `.sock` files from a dead pid are removed, not just ignored.

## Phase 7 — live Coder validation matrix (`#coder7`, operator-verify)

### Objective

Prove the whole path on a real workspace; the earlier phases are testable
offline but the integration is not.

### Matrix

| # | Client | State | Expectation |
|---|---|---|---|
| 1 | JB Toolbox/Gateway remote backend | cold workspace, no tmux | tab opens, session created, Run round-trips |
| 2 | VS Code Remote-SSH | cold workspace | same |
| 3 | Both attached to one workspace | one document open in each | two editor endpoints, CRDT replicas converge, no duplicate panes |
| 4 | Workspace stop → start | prior session existed | stale rows GC'd, one fresh session, no wedge |
| 5 | Workspace image without tmux | — | dialog names the image fix, no crash |

### Verification

Record outcomes in this file; only then close `#coder7`.

## Risks / open items

- **Env-detection guesswork.** Coder's env var names, the JB remote-backend
  probe, and `remoteName` values are all asserted from general knowledge here,
  not read out of a live workspace. Phase 1 must verify them first; everything
  downstream keys off that classification.
- **JB terminal API stability.** `TerminalToolWindowManager` has moved packages
  across platform versions and the new (reworked) terminal changes widget
  access. Pin against the plugin's existing `242+` target and keep the dialog
  fallback on any `NoClassDefFoundError`.
- **Coder autostop mid-turn.** A workspace stopped while a turn is in flight
  leaves an uncommitted cycle. Phase 6 covers reconciliation on next boot, but
  whether the existing resume-commit path handles a *whole-host* kill is
  unverified — check it during Phase 7 case 4.
- **No `agent-doc terminal` regression.** The GUI-terminal path stays the
  `external` host and must keep working unchanged for the current laptop
  workflow.
