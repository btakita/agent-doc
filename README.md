# agent-doc

**Your markdown document is the conversation.**

agent-doc turns an ordinary markdown file into a live session with an AI coding
agent. You write in the document, press a hotkey (or just keep typing), and the
agent reads what you changed and writes its answer back into the same file.
Nothing scrolls away: you can reorganize, annotate, and prune the conversation
like any other document, and every turn is committed to git.

> **Alpha software.** Actively developed; commands and frontmatter may change
> between versions. **Single-user only**: use a private git repository (see
> [Security](#security)).

## Why a document?

Terminal chats are ephemeral. A document is a workspace you curate:

- **Edits are the prompt.** agent-doc diffs the document against the last turn,
  so the agent sees exactly what you added, changed, or deleted, including
  inline notes on its own earlier answers.
- **Work queues up.** Drop several requests into an `agent:run` section and
  the agent drains them one by one; track follow-ups in `agent:backlog`.
- **You can keep typing.** Edits made while the agent works merge safely with
  its writes in real time, inside your editor, without clobbering your cursor
  or undo history.
- **History is real history.** Every turn is a git commit you can diff, revert,
  or squash.

## How it works

1. You edit a session document (a markdown file with agent-doc frontmatter).
2. A hotkey, your editor plugin, or your agent harness triggers a turn.
3. agent-doc computes the diff since the last turn and hands it to the agent,
   which runs in its own tmux pane.
4. The agent's response is written back into the document (appended, or patched
   into named sections like `agent:exchange`), and the cycle is committed.

The Rust binary owns every deterministic step (diffing, merging, routing, git);
the bundled skill instructions tell the agent how to take part. See
[Architecture](docs/reference/architecture.md) for the full picture.

## Works with

**Editors**

| Editor | Integration |
|--------|-------------|
| JetBrains IDEs (IntelliJ IDEA, Rider, PyCharm, and other IntelliJ-platform IDEs) | Plugin: `agent-doc plugin install jetbrains`, or the JetBrains Marketplace |
| VS Code | Extension: `agent-doc plugin install vscode`, or the VS Code Marketplace |
| Zed | Dev extension in [`editors/zed`](editors/zed/README.md) |
| No editor plugin | Drive sessions directly from your agent harness, or bind `agent-doc run <file>` to a hotkey in any editor (Vim/Neovim included) |

The plugins add realtime sync, tmux layout mirroring (one pane per visible
document), and highlighting for agent-doc structures. See
[Editor Integration](docs/guide/editor-integration.md).

**Agent harnesses**

| Harness | How you start a turn |
|---------|---------------------|
| Claude Code | `/agent-doc <file>` |
| Codex | `agent-doc <file>` as a normal message |
| Grok Build | `agent-doc <file>` as a normal message (uses the installed MCP server) |
| OpenCode | `/agent-doc <file>` |

`agent-doc skill install --harness <claude|codex|grok|opencode>` installs the
matching instructions (or `--all` for every harness; Cursor rules and a generic
target are also available). To pick which agent a document's pane runs, set
`agent:` in its frontmatter (`claude`, `codex`, `grok`, or `opencode`). See [Agent Backends](docs/guide/agent-backends.md).

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/btakita/agent-doc/main/install.sh | sh
```

Or `pip install agent-doc`, `brew install btakita/tap/agent-doc`, or a prebuilt
binary from [GitHub Releases](https://github.com/btakita/agent-doc/releases).
Then add your editor plugin and harness instructions:

```sh
agent-doc plugin install jetbrains        # or: vscode
agent-doc skill install --harness claude  # or: codex, grok, opencode, --all
```

agent-doc's managed sessions run agents in tmux, so install `tmux` too.
Source builds, PyPI bootstrap details, Windows, and upgrades are covered in
[Installation](docs/getting-started/installation.md).

## Your first session

```sh
cd your-project
agent-doc init                       # set up .agent-doc/ for this project
agent-doc init notes.md "My Topic"   # scaffold a session document
```

Open `notes.md`, write a request, and start a turn:

- **From an editor plugin:** run **Run Agent Doc** (JetBrains) or
  **Agent Doc: Run (Submit)** (VS Code) on the file. agent-doc starts (or
  reuses) the document's agent pane and sends the turn.
- **From your harness:** type `/agent-doc notes.md` in Claude Code or OpenCode,
  or `agent-doc notes.md` in Codex or Grok Build.
- **From a terminal:** `agent-doc start notes.md` opens a managed agent pane for
  the document, and `agent-doc run notes.md` runs a single headless turn.

The response lands in `notes.md`. Keep writing below it and run again. For a
guided tour, see the [Quick Start](docs/getting-started/quick-start.md); for
multi-pane tmux setups, remote workspaces, and running two IDEs on one project,
see [Tmux Routing and Navigation Lanes](docs/guide/tmux-routing.md).

## Queue markers

agent-doc prefixes each row in an `agent:run` section with its state:

| Marker | Meaning |
|--------|---------|
| `📥` | pending, ready to run |
| `🟡` | claimed for a worker that has not started yet |
| `🚧` | a worker is running it |
| `☑️` | the worker reported back; the coordinator still has to integrate and close it |
| `⚠️` | lost: the claim expired; it can be reclaimed |
| `🔄` | needs reconcile: there is finished-work evidence but no current claim; it is not dispatched again |
| `❔` | unknown: the state is unreadable or contradicts the row |
| `⏭️` | skipped this pass |
| `⏳` | waiting on an operator verdict |
| `🙋` | an operator request is open for the row |
| `📌` / `📍` | operator / agent priority pin |

A session document with an `agent:run` section keeps this legend on one line
at the end of its `agent:status` section (hide it with `queue-legend=off` on
the status marker), and `agent-doc queue --help` prints it. See
[Queue Markers](docs/guide/queue-markers.md#queue-markers) for the full legend,
including the `[#owner#N]` and `⟦effect:N⟧` annotations.

## Learn more

- [Documentation home](docs/README.md) (also built as an mdBook)
- [Document Format](docs/guide/document-format.md) and
  [Components](docs/guide/components.md): frontmatter, `agent:` sections, queue
  and backlog
- [Tags and Opening-Marker Attributes](docs/reference/tags-and-attributes.md):
  complete `agent:*`, response patch, boundary, and attribute reference
- [Commands](docs/guide/commands.md) and [Configuration](docs/getting-started/configuration.md)
- [Feature Catalog](docs/reference/features.md): the detailed capability ledger
- [Architecture](docs/reference/architecture.md): binary vs. skill
  responsibilities and the domain ontology
- [SPEC.md](SPEC.md) and [`specs/`](specs/07-commands.md): the functional
  specification

## Security

agent-doc is designed for **single-user, local operation**. Documents,
snapshots, and exchange history live on your filesystem and are committed to a
git repository.

- **Single user only.** There is no multi-user access control, authentication,
  or session isolation.
- **Use a private repo.** Session documents may contain sensitive content
  (correspondence, research, credentials in context).
- **Prompt injection risk.** Content pasted from emails, web pages, or chat logs
  could contain prompt injection. The agent treats all document content as user
  input, with no injection scanning.
- **`--dangerously-skip-permissions` exposure.** With this flag (common in
  agent-doc sessions) the agent has full filesystem access, so injected prompts
  could read files or run commands if not sandboxed.
- **The shared-doc guard is an audit marker, not auth.** Documents may opt into
  `agent_doc_collaboration: shared`; then cross-document `extract` / `transfer`
  and plan-backed `do #id` work that references another `.md` file fail closed
  unless the document also carries `agent_doc_security_review: <review-id>`.

**Planned:** collaborative security for web/networked deployments (multi-user
access control, session isolation, content scanning, compartmented access).

## License

Licensed under the [Apache License, Version 2.0](LICENSE) (`Apache-2.0`).
