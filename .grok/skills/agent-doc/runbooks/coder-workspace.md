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
- VS Code Remote SSH/Coder: install the extension on the remote extension host.
  `vscode.env.remoteName` is passed to the binary as an opaque IDE observation.
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
