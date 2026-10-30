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

## Getting started

From a clean machine to your first agent turn. Each step links to the full
guide if you need more.

**You need:** `git`, `tmux`, and one agent CLI that already works on its own
in a terminal: [Claude Code](https://docs.anthropic.com/en/docs/claude-code)
(`claude`), Codex, Grok Build, or OpenCode. agent-doc drives that agent; it does
not replace it or hold your API keys.

**1. Install the CLI.**

```sh
curl -fsSL https://raw.githubusercontent.com/btakita/agent-doc/main/install.sh | sh
agent-doc --version
```

`pip install agent-doc` and `brew install btakita/tap/agent-doc` also work. If
the installer prints a `PATH` note, add that line to your shell profile. See
[Installation](docs/getting-started/installation.md).

**2. Set up your project** (any git repository):

```sh
cd your-project
agent-doc init
agent-doc skill install --harness claude   # or: codex, grok, opencode, --all
```

`agent-doc init` creates `.agent-doc/` (local runtime state: add it to
`.gitignore`) plus `runbooks/` and `okf/` reference files for the agent.
`skill install` teaches your agent the agent-doc workflow by writing its
instructions where that agent looks, for example `.claude/skills/agent-doc/`
for Claude Code. Run `skill install` with your harness even if `init` already
installed one: from a plain terminal, `init` cannot tell which agent you use.
The instruction files are regenerated on upgrade; commit them if teammates use
agent-doc in the same repository.

**3. Optional: add the editor plugin.**

```sh
agent-doc plugin install jetbrains   # or: vscode
```

Restart the IDE if it asks you to. The plugin gives you a **Run Agent Doc** action,
live merging while the agent writes, and highlighting. Without a plugin,
everything below still works from a terminal. See
[Editor Integration](docs/guide/editor-integration.md).

**4. Create your first session document.**

```sh
agent-doc init notes.md "My Topic"
```

Open `notes.md` and write a request at the end, under `## User`. For example,
"Summarize what this repository does."

**5. Run the first turn.** Pick whichever fits how you work:

| Where you are | What to do |
|---|---|
| Your agent (Claude Code, OpenCode) | type `/agent-doc notes.md` |
| Your agent (Codex, Grok Build) | type `agent-doc notes.md` as a normal message |
| JetBrains / VS Code with the plugin | run **Run Agent Doc** (JetBrains) or **Agent Doc: Run (Submit)** (VS Code) on the file |
| A terminal | `agent-doc run notes.md` for one headless turn, or `agent-doc start notes.md` for a long-lived agent pane in tmux |

The answer is written into `notes.md` as a `## Assistant` block and the turn is
committed to git. Add another `## User` block below it and run again; that is
the whole loop. `agent-doc run notes.md --dry-run` shows what the agent would
receive without sending anything.

**What the markers mean.** `<!-- agent:NAME -->` ... `<!-- /agent:NAME -->`
pairs are named sections (components) that agent-doc reads and patches, such as
`agent:exchange` for the conversation or `agent:run` for a work queue. You do
not need any for a first session. The icons agent-doc adds to queue items are
explained under [Queue attributes](docs/reference/tags-and-attributes.md#queue-attributes).
See [Document Format](docs/guide/document-format.md) and
[Components](docs/guide/components.md) for the full layout.

**If something goes wrong:** `agent-doc version` shows which binary, native
library, and plugin versions are in use, and `agent-doc env` reports what
agent-doc sees of your terminal and tmux. A step-by-step version of this path,
with troubleshooting, lives in the
[Quick Start](docs/getting-started/quick-start.md).

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

## Queue markers

agent-doc prefixes each row in an `agent:run` section with its state:

| Marker | Meaning |
|--------|---------|
| `📥` | pending, ready to run |
| `⛔` | blocked: its tracked item waits on open work, named after the marker (`⛔ ⟦blocked by a, b⟧`); it is not drained or dispatched until that work closes |
| `🟡` | claimed for a worker that has not started yet |
| `🚧` | a worker is running it |
| `☑️` | keep the row: the worker reported back, but the coordinator still has to integrate and close it |
| `⚠️` | lost: the claim expired; it can be reclaimed |
| `🔄` | needs reconcile: there is finished-work evidence but no current claim; it is not dispatched again |
| `❔` | unknown: the state is unreadable or contradicts the row |
| `⏭️` | skipped this pass |
| `⏳` | waiting on an operator verdict |
| `🙋` | an operator request is open for the row |
| `📌` / `📍` | operator / agent priority pin |
| `~~row~~` | integrated and terminal; a prompt tree is safe to delete only when every row is struck |

A session document with an `agent:run` section keeps this legend on one line
at the end of its `agent:status` section (hide it with `queue-legend=off` on
the status marker), and `agent-doc queue --help` prints it. See
[Queue Markers](docs/guide/queue-markers.md#queue-markers) for the full legend,
including the `[#owner#N]`, `⟦blocked by …⟧` and `⟦effect:N⟧` annotations.
Completed queue rows also retain `⟦addressed-at:…⟧`; supported editors rank
the newest three as `①`/`②`/`③` with graded, caret-dismissible highlights.

## Learn more

- [Documentation home](docs/README.md) (also built as an mdBook)
- [Document Format](docs/guide/document-format.md) and
  [Components](docs/guide/components.md): frontmatter, `agent:` sections, queue
  and backlog
- [Tags and Opening-Marker Attributes](docs/reference/tags-and-attributes.md):
  complete `agent:*`, response patch, boundary, and attribute reference
- [Commands](docs/guide/commands.md) and [Configuration](docs/getting-started/configuration.md)
- [Tmux Routing and Navigation Lanes](docs/guide/tmux-routing.md): multi-pane
  tmux setups, remote workspaces, and two IDEs on one project
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
