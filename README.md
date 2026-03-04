# agent-doc

> **Alpha Software** — actively developed, APIs and frontmatter format may change between versions. Feedback welcome via GitHub issues.

Interactive document sessions with AI agents.

Edit a markdown document. Press a hotkey. The tool diffs your changes, sends
them to an AI agent, and writes the response back into the document. The
document is the UI.

## Why

Terminal prompts are ephemeral. You type, the agent responds, the context
scrolls away. Documents are persistent — you can reorganize, delete noise,
annotate inline, and curate the conversation as a living artifact. The agent
sees your edits as diffs, so every change carries intent.

## Install

```sh
cargo install --path .
```

## Quick Start

```sh
agent-doc init session.md "Topic Name"    # scaffold a session doc
agent-doc run session.md                  # diff, send, append response
agent-doc diff session.md                 # preview what would be sent
agent-doc reset session.md                # clear session + snapshot
agent-doc clean session.md                # squash session git history
agent-doc route session.md               # route to tmux pane (or auto-start)
agent-doc start session.md               # start Claude in current tmux pane
agent-doc claim session.md [--window W]  # claim file for a tmux pane
agent-doc focus session.md               # focus tmux pane for a session
agent-doc layout a.md b.md --split h [--window W]  # arrange panes (window-scoped)
agent-doc outline session.md             # section structure + token counts
agent-doc outline session.md --json      # JSON output for tooling
agent-doc resync                         # validate sessions, remove dead panes
agent-doc commit session.md              # git add + commit with timestamp
agent-doc prompt session.md              # detect permission prompts → JSON
agent-doc skill install                  # install Claude Code skill definition
agent-doc skill check                    # check if skill is up to date
agent-doc upgrade                        # upgrade to latest version
```

## Document Format

```markdown
---
agent_doc_session: 05304d74-90f1-46a1-8a79-55736341b193
agent: claude
---

# Session: Topic Name

## User

Your question or instruction here.

## Assistant

(agent writes here)

## User

Follow-up. You can also annotate inline:

> What about edge cases?
```

### Frontmatter fields

| Field | Required | Default | Description |
|-------|----------|---------|-------------|
| `agent_doc_session` | no | (generated on first run) | Document UUID for tmux pane routing (legacy: `session`) |
| `resume` | no | (none) | Claude conversation ID for `--resume` |
| `agent` | no | `claude` | Agent backend to use |
| `model` | no | (agent default) | Model override |
| `branch` | no | (none) | Git branch for session commits |

### Two interaction modes

**Append mode:** Structured `## User` / `## Assistant` blocks.

**Inline mode:** Annotations anywhere — blockquotes, edits to previous
responses. The diff captures what changed; the agent addresses inline edits
alongside new `## User` content.

Both work simultaneously because the run sends a diff, not a parsed structure.

## Run Flow

```
┌──────────┐  hotkey ┌────────────┐  diff + prompt  ┌───────┐
│  Editor  │ ──────> │ agent-doc  │ ──────────────> │ Agent │
│          │         │            │ <────────────── │  API  │
│  reload  │ <────── │ write+snap │                 └───────┘
└──────────┘         │ git commit │
                     └────────────┘
```

1. Read document, load snapshot (last-known state)
2. Compute diff — if empty, exit (double-run guard)
3. Send diff + full document to agent, resuming session
4. Append response as `## Assistant` block
5. Save snapshot, git commit

### Session continuity

- **Empty `agent_doc_session:`** — forks from the most recent agent session in the
  directory (inherits context)
- **`agent_doc_session: <uuid>`** — resumes that specific session
- **Delete `agent_doc_session:` value** — next run starts fresh

### History rewriting

Delete anything from the document. On next run, the diff shows deletions
and the agent sees the cleaned-up doc as ground truth.

## Git Integration

Each run auto-commits the document for inline diff highlighting in your editor.

| Flag | Behavior |
|------|----------|
| `-b` | Auto-create branch `agent-doc/<filename>` on first run |
| (none) | Commit to current branch |
| `--no-git` | Skip git entirely |

Cleanup: `agent-doc clean <file>` squashes all session commits into one.

## Agent Backends

Agent-agnostic core. Only the "send prompt, get response" step varies.

```toml
# ~/.config/agent-doc/config.toml

[agents.claude]
command = "claude"
args = ["-p", "--output-format", "json"]
result_path = ".result"
session_path = ".session_id"

[agents.codex]
command = "codex"
args = ["--prompt"]
result_path = ".output"
session_path = ".id"

default_agent = "claude"
```

Override per-document via `agent:` in frontmatter, or per-invocation via `--agent`.

## Tmux Session Routing

Route documents to persistent Claude sessions via tmux. Pane management is powered by [tmux-router](https://github.com/btakita/tmux-router).

```sh
agent-doc route plan.md    # send to existing pane, or auto-start one
agent-doc sync --col a.md,b.md --col c.md --focus a.md  # 2D layout sync
```

**How it works:**
1. Each document gets an `agent_doc_session` UUID in frontmatter (auto-generated if missing)
2. agent-doc maps UUIDs to file paths, then delegates to tmux-router for pane routing
3. `route` checks if the pane is alive — if so, sends the command; if dead, auto-starts a new one
4. `sync` mirrors editor split layout in tmux using attach-first reconciliation

## Editor Integration

### JetBrains

External Tool: Program=`agent-doc`, Args=`run $FilePath$`,
Working dir=`$ProjectFileDir$`, Output paths=`$FilePath$`. Assign keyboard shortcut.

### VS Code

Task: `"command": "agent-doc run ${file}"`. Bind to keybinding.

### Vim/Neovim

```vim
nnoremap <leader>as :!agent-doc run %<CR>:e<CR>
```

## CLI Reference

```
agent-doc run <file> [-b] [--agent <name>] [--model <model>] [--dry-run] [--no-git]
agent-doc init <file> [title] [--agent <name>]
agent-doc diff <file>
agent-doc reset <file>
agent-doc clean <file>
agent-doc route <file>              # route to existing tmux pane or auto-start
agent-doc start <file>              # start Claude session in current tmux pane
agent-doc claim <file> [--window W] [--pane P]  # claim file for a tmux pane
agent-doc focus <file> [--pane P]              # focus tmux pane for a session
agent-doc layout <files> --split h [--window W] # arrange panes (window-scoped)
agent-doc outline <file> [--json]    # section structure + token counts
agent-doc resync                    # validate sessions, remove dead panes
agent-doc prompt <file> [--all]     # detect permission prompts → JSON
agent-doc prompt --answer N <file>  # answer prompt option N
agent-doc commit <file>             # git add + commit with timestamp
agent-doc skill install             # install Claude Code skill definition
agent-doc skill check               # check if installed skill is up to date
agent-doc audit-docs                # audit instruction files for staleness
agent-doc upgrade                   # upgrade to latest version
```

## License

MIT