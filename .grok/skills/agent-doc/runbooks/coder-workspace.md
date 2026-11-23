# Coder workspace terminals

Use this runbook when agent-doc runs in a Coder workspace, JetBrains Gateway,
VS Code Remote SSH, or another remote IDE backend.

## Workspace image requirements

Install both `tmux` and `agent-doc` in the workspace image. A bootstrap script
may call `agent-doc start <FILE>`; outside tmux, start creates the project-bound
session and re-enters it before launching the supervisor. An image without tmux
fails closed with an image-remediation message.

Do not put a second session name under `[terminal]`. Set the project binding with
`agent-doc session set <NAME>` (stored as top-level `tmux_session` in
`.agent-doc/config.toml`).

## Host policy

Terminal presentation is separate from tmux session ownership:

```toml
# ~/.config/agent-doc/config.toml or .agent-doc/config.toml
[terminal]
host = "auto"              # auto | ide | external | none
auto_start_tmux = true
command = "wezterm start -- {tmux_command}"
attach_command = "tmux attach-session -t {session}"
```

`terminal_host` in document frontmatter overrides the project host, which
overrides the global host. The other `[terminal]` fields use project then global
fallback. With `host = "auto"`, an available remote IDE terminal wins over an
external terminal. `host = "none"` keeps the tmux session headless.

The binary is the policy owner. `agent-doc tmux ensure <FILE> --ide-terminal
--json` returns the resolved host, reason, attach command, and session state;
plugins consume that receipt instead of reimplementing environment detection.

## Attach behavior

An existing attached tmux client is authoritative. Neither JetBrains nor VS Code
opens a second client in that case. If the session is detached and the resolved
host is `ide`, the plugin reuses its matching terminal tab when possible, then
creates one with the receipt's attach command.

- JetBrains Gateway: install the agent-doc plugin in the remote backend. The
  backend terminal hosts the tmux client; the local Gateway UI is not treated as
  an external terminal.
- JetBrains Gateway layout sync (GH #434): the editor split geometry that tmux
  pane auto-sync, Sync Tmux Layout, and Run Agent Doc follow is measured by the
  plugin's frontend module, which runs in the JetBrains Client on the
  operator's machine, not in the Coder workspace. `agent-doc plugin install
  jetbrains` on the workspace installs only the backend side; with a local ZIP
  or custom repository the Client gets nothing. Install the same modular ZIP
  (`agent-doc-jetbrains-262-<version>.zip`) in the JetBrains Client/Gateway
  plugins on the local machine, confirm both sides report that exact version,
  and restart both, including the Client process. Until then the backend
  `idea.log` shows `[layout-detect] unknown windows=0
  source=remote_client_selected_files ...
  reason=flat_evidence_has_no_column_geometry` and `[layout-sync] frontend
  layout publisher unavailable`, focus defers with
  `focus_deferred:no_frontend_layout_snapshot surface_id=surface-unavailable`,
  and Run Agent Doc still dispatches into the document's existing pane but
  logs `[route] layout not synced for <doc>: [route] layout-not-synced: ...`
  with the reason instead of moving tmux columns. Local (non-Gateway) IDEs
  are unaffected.
- Upgrading on Coder (GH #435): `agent-doc upgrade` in the workspace updates
  only the backend's plugin. It prints the Client steps (download URL, quit
  the Client, delete `agent-doc-jetbrains-262` and `agent-doc-jetbrains-263`
  from the Client plugins directory, extract the ZIP, restart). Until the
  Client matches, the backend logs `[split-surface] frontend/backend plugin
  mismatch` and shows a warning in the Client.
- JetBrains Gateway with the frontend installed (GH #435): once the Client's
  frontend opens its lease, the backend logs `reason=frontend_measured_layout_authority`
  and `[layout-detect] unknown windows=0 ... flat_evidence_has_no_column_geometry`
  by design. The backend has no editor windows of its own; the geometry travels
  Client frontend -> backend RPC -> controller `editor_view_snapshot_observe`
  -> `[rd-layout] forwarded measured columns`. Read the hop that stopped from:
  - Client `idea.log`: `[split-surface] frontend surface publisher started`
    (module loaded), `frontend/backend ingress connected generation=<n>`,
    `snapshot schema negotiated version=<n> ... geometry=measured`, then
    `snapshot accepted ... measured_windows=<a>/<b>` or
    `snapshot rejected status=<STATUS> ... retry_in_ms=<ms> diagnostic=<why>`.
    A recoverable refusal (`DELIVERY_FAILED`, `PROJECT_MISMATCH`,
    `INCOMPLETE`) republishes on a 1-30 s backoff.
  - Backend `idea.log`: `[split-surface] frontend/backend ingress accepted`
    (or `deferred`), `lease opened ... layout_authority=remote_measured`,
    `snapshot refused ... status=<STATUS> diagnostic=<why>`,
    `[split-surface] controller delivery failed`, and 5 s after an
    unpublished lease `[layout-sync] frontend layout publisher unavailable
    ... readiness_reason=frontend_connected_without_accepted_publication
    ... last_refusal=<STATUS>:<why>`. A route that defers names each lease's
    state: `[route] surface reference unavailable ...
    layout_authority=frontend_measured frontend_layout=[remote-client:<id>:<gen>:<state>]`
    (`awaiting_first_snapshot`, `snapshot_refused(<STATUS>)`,
    `measured_layout_unavailable(<reason>)`, `forward_unavailable(...)`, or
    `forwarded(columns=<n>,sequence=<n>)`).
  - `diagnostic=Timed out waiting for 2000 ms` together with route failures
    `Project Controller did not respond within 10000ms`, and a non-blocking
    connect to `.agent-doc/controller.sock` returning `EAGAIN`, means the
    controller's accept backlog is full. Builds before `2d4156fee7` counted
    parked long-poll clients against the 64 short-RPC slots. Upgrade, or
    recover with `agent-doc admin recycle --project-root <root>`.
- VS Code Remote SSH/Coder: the extension declares
  `"extensionKind": ["workspace"]`, so it runs in the remote extension host.
  Run `agent-doc plugin install vscode` on the workspace: it installs into
  `code-server` when that is on `PATH` (otherwise `~/.local/bin/code-server`
  or `/tmp/code-server/bin/code-server`, where Coder's code-server module
  installs it), into Coder's VS Code Web (the `vscode-web` module, which
  extracts the VS Code Server web build into `/tmp/vscode-web` by default;
  `~/.vscode-web` is also probed) through its own
  `bin/code-server`, and into the newest Remote-SSH server
  under `~/.vscode-server` (or `$VSCODE_AGENT_FOLDER`), `~/.vscodium-server` or
  `~/.cursor-server` through the server's own CLI. That server exists only
  after VS Code has connected once. Reload the remote window after an install.
  VS Code Web loads extensions from `~/.vscode-server/extensions` (or
  `$VSCODE_AGENT_FOLDER/extensions`) unless the template sets the module's
  `extensions_dir`, or a hand-started server passes `--server-data-dir`.
  While the server runs, a custom `install_prefix`, `extensions_dir`,
  `--server-data-dir`, or that process's own `VSCODE_AGENT_FOLDER` needs
  nothing extra: the install reads them from the `serve-local` process, not
  from the shell that runs the install, and `agent-doc plugin list` names
  the directory. With the server stopped, start the workspace first or
  install the VSIX by hand:
  `<install_prefix>/bin/code-server --extensions-dir <extensions_dir> --install-extension <vsix>`.
  `vscode.env.remoteName` is passed to the binary, and to the `agent-doc`
  terminal the extension creates, as an opaque IDE observation.
- External terminal: configure `host = "external"` only where a display/session
  and terminal command actually exist. Explicit external hosting in a headless
  Coder workspace fails closed rather than launching locally or falling back to
  an IDE silently.

Set `auto_start_tmux = false` to require an operator-created session. A missing
session then produces a manual-start error; an existing detached session may
still be attached according to the resolved host.

## Diagnostics

Run these from the workspace backend:

```sh
agent-doc env --json
agent-doc tmux ensure path/to/session.md --ide-terminal --json
agent-doc session status path/to/session.md
```

Check that Coder reports a workspace identity, the IDE observation matches the
remote extension/backend, the receipt resolves the expected host, and the
project `tmux_session` is the one being attached.
`agent-doc reliable-sync-status` lists each VS Code registration with its
activation receipt (`activation: role=workspace generation=<n> ...`).

## VS Code remote smoke test

Run `scripts/vscode-remote-smoke.py` from the agent-doc checkout, on the
workspace host, with the project and one session document. Steps 2-6 need an
operator with VS Code connected to the workspace; a headless agent can only
run `--steps install`, and a timed-out wait with nobody connected is not a
product failure.

1. Pick a real session document: one with agent-doc frontmatter or matched by
   `[documents] include`. The extension registers only session documents, so
   the script exits 2 before any step when `--doc` is not one (for example
   `README.md`). Run the smoke against the project the VS Code window has
   open, not a fresh worktree nobody opened.
2. Make sure `.agent-doc/config.toml` names the project's `tmux_session`
   (`agent-doc session set <NAME>`); tmux-attach fails without it.
3. Connect VS Code to the workspace (Coder Remote / Remote-SSH, or the
   code-server or VS Code Web browser tab) and open the document.
4. Run, with the default 300 s waits:

```sh
python3 scripts/vscode-remote-smoke.py --project ~/project --doc tasks/plan.md
```

5. When the install step reports it, reload the remote window (**Developer:
   Reload Window**), then run **Agent Doc: Run** or **Claim** so the extension
   attaches its `agent-doc` terminal, and **Agent Doc: Sync Layout**.

`--install release` (the default) installs the latest released extension,
`--install local` the checkout's VSIX build, and `--install skip` keeps what
is installed. It checks, and prints PASS, FAIL or SKIP for, each step:
install into a workspace host with `extensionKind` `workspace`; a VS Code
registration with a `role=workspace` receipt; that the registered pid is a
server extension host; a tmux client on the project session under the same
server; `AGENT_DOC_VSCODE_REMOTE_NAME` in that client; and an accepted surface
observation with no pane divergence. A step whose prerequisite did not pass
is skipped, so fix the first FAIL. A timed-out wait names whether any server
extension host process is running: none means no VS Code window is connected
to this host. It exits 1 when any step failed. `--steps` selects steps and
`--timeout` bounds each wait.
