# Quick Start

The shortest path from a clean machine to a working agent-doc session. The
[README](../../README.md#getting-started) has the same steps in brief; this
page adds what each step does and what to check when one fails.

## Before you start

You need:

- `git`, and a git repository to work in. agent-doc commits each turn.
- `tmux`. agent-doc runs long-lived agent panes in it. No tmux server has to be
  running; agent-doc starts one when it needs it.
- One agent CLI that already works on its own: Claude Code (`claude`), Codex,
  Grok Build, or OpenCode. Start it once by itself and finish its login first;
  agent-doc drives it but does not hold your credentials.

## 1. Install the CLI

```sh
curl -fsSL https://raw.githubusercontent.com/btakita/agent-doc/main/install.sh | sh
agent-doc --version
```

The installer puts `agent-doc` and its native library (`libagent_doc.so`, or
`.dylib` on macOS) in `~/.cargo/bin` if that exists, otherwise `~/.local/bin`.
If it prints a `PATH` note, add that line to your shell profile and open a new
terminal. Other install methods are in [Installation](installation.md).

## 2. Set up your project

```sh
cd your-project
agent-doc init
agent-doc skill install --harness claude
```

- `agent-doc init` checks prerequisites, creates `.agent-doc/`, and writes the
  agent-facing `runbooks/` and `okf/` reference files. `.agent-doc/` holds local
  runtime state (a database, sockets, logs); add it to `.gitignore`.
- `agent-doc skill install --harness <name>` installs the agent-doc workflow
  instructions where your agent reads them. Use `claude`, `codex`, `grok`, or
  `opencode`, or `--all`. From a plain terminal `init` cannot tell which agent
  you use, so it installs a generic copy and prints this command for you.

## 3. Optional: install an editor plugin

```sh
agent-doc plugin install jetbrains
```

Use `vscode` instead of `jetbrains` for VS Code (the `code`, `codium`, or
`cursor` command must be on `PATH`). Restart the IDE if it asks you to. The
plugin adds a **Run Agent Doc** action, live merging while the agent writes,
and highlighting. Everything below also works without a plugin. See
[Editor Integration](../guide/editor-integration.md) for other install routes
and hotkey-only setups.

## 4. Create a session document

```sh
agent-doc init notes.md "My Topic"
```

This creates:

```markdown
---
agent_doc_session: 959a0bfe-ce35-48a2-90bb-03f5e6c38cf2
agent: claude
---

# Session: My Topic

## User

```

Write your request at the end, under `## User`:

```markdown
## User

Summarize what this repository does.
```

Set `agent:` to `codex`, `grok`, or `opencode` to use a different agent for this
document. `--mode template` creates the `agent:exchange` layout used by longer
sessions instead; see [Document Format](../guide/document-format.md).

## 5. Run the first turn

Choose one:

- **In your agent:** type `/agent-doc notes.md` in Claude Code or OpenCode, or
  `agent-doc notes.md` as a normal message in Codex or Grok Build.
- **In the IDE plugin:** run **Run Agent Doc** (JetBrains) or
  **Agent Doc: Run (Submit)** (VS Code) with `notes.md` open.
- **In a terminal:** run one headless turn:

  ```sh
  agent-doc run notes.md
  ```

  or open a long-lived agent pane for the document in tmux with
  `agent-doc start notes.md` (it prints the `tmux attach` command to watch it).

To see what would be sent without sending it:

```sh
agent-doc run notes.md --dry-run
```

agent-doc diffs the document against the previous turn, sends the change to
the agent, writes the answer back as a `## Assistant` block, and commits the
turn. Add a new `## User` block below the answer and run again. That is the
whole loop.

## What you will see in a document

- `## User` / `## Assistant` blocks in the default (append) layout.
- `<!-- agent:NAME -->` ... `<!-- /agent:NAME -->` pairs: named sections
  (components) that agent-doc reads and patches, such as `agent:exchange` or
  the `agent:run` work queue. See [Components](../guide/components.md).
- Status icons on queue items, which agent-doc maintains for you. They are
  explained under
  [Queue attributes](../reference/tags-and-attributes.md#queue-attributes).

## When something does not work

| Symptom | What to do |
|---|---|
| `agent-doc: command not found` | The install directory is not on `PATH`; add the line the installer printed and open a new terminal. |
| `/agent-doc` is not a known command in your agent | Run `agent-doc skill install --harness <your agent>` in the project root, then restart the agent session. |
| `init` reports `tmux ... MISSING` | Install tmux: <https://github.com/tmux/tmux/wiki/Installing>. |
| `init` reports `tmux ... BROKEN` | A tmux server is running but this `tmux` client cannot talk to it (usually two tmux versions). Set `tmux_bin` in `~/.config/agent-doc/config.toml` to the client that owns the server. |
| `plugin install jetbrains` finds no plugins directory | Start the IDE once so it creates its data directory, or pass `--plugins-dir` (see [Editor Integration](../guide/editor-integration.md)). |
| The plugin reports degraded or file-based IPC | `agent-doc version` must show a `native library` path. If it says `not found`, reinstall with the installer above, which puts the library next to the binary. |
| The turn starts but the agent asks you to log in | Finish the agent's own first-run setup by starting it once on its own. |

`agent-doc version` prints the binary, native library, and expected plugin
versions; `agent-doc env` prints what agent-doc sees of your terminal and tmux.
Include both when you ask for help.
