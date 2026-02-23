# agent-doc

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
agent-doc submit session.md               # diff, send, append response
agent-doc diff session.md                 # preview what would be sent
agent-doc reset session.md                # clear session + snapshot
agent-doc clean session.md                # squash session git history
```

## Document Format

```markdown
---
session: 05304d74-90f1-46a1-8a79-55736341b193
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
| `session` | no | (generated on first submit) | Session ID for continuity |
| `agent` | no | `claude` | Agent backend to use |
| `model` | no | (agent default) | Model override |
| `branch` | no | (none) | Git branch for session commits |

### Two interaction modes

**Append mode:** Structured `## User` / `## Assistant` blocks.

**Inline mode:** Annotations anywhere — blockquotes, edits to previous
responses. The diff captures what changed; the agent addresses inline edits
alongside new `## User` content.

Both work simultaneously because the submit sends a diff, not a parsed structure.

## Submit Flow

```
┌──────────┐  hotkey  ┌────────────┐  diff + prompt  ┌───────┐
│  Editor  │ ──────> │ agent-doc  │ ──────────────> │ Agent │
│          │         │            │ <────────────── │  API  │
│  reload  │ <────── │ write+snap │                 └───────┘
└──────────┘         │ git commit │
                     └────────────┘
```

1. Read document, load snapshot (last-known state)
2. Compute diff — if empty, exit (double-submit guard)
3. Send diff + full document to agent, resuming session
4. Append response as `## Assistant` block
5. Save snapshot, git commit

### Session continuity

- **Empty `session:`** — forks from the most recent agent session in the
  directory (inherits context)
- **`session: <uuid>`** — resumes that specific session
- **Delete `session:` value** — next submit starts fresh

### History rewriting

Delete anything from the document. On next submit, the diff shows deletions
and the agent sees the cleaned-up doc as ground truth.

## Git Integration

Each submit auto-commits the document for inline diff highlighting in your editor.

| Flag | Behavior |
|------|----------|
| `-b` | Auto-create branch `agent-doc/<filename>` on first submit |
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

## Editor Integration

### JetBrains

External Tool: Program=`agent-doc`, Args=`submit $FilePath$`,
Working dir=`$ProjectFileDir$`, Output paths=`$FilePath$`. Assign keyboard shortcut.

### VS Code

Task: `"command": "agent-doc submit ${file}"`. Bind to keybinding.

### Vim/Neovim

```vim
nnoremap <leader>as :!agent-doc submit %<CR>:e<CR>
```

## CLI Reference

```
agent-doc submit <file> [-b] [--agent <name>] [--model <model>] [--dry-run] [--no-git]
agent-doc init <file> [title] [--agent <name>]
agent-doc diff <file>
agent-doc reset <file>
agent-doc clean <file>
```

## License

MIT