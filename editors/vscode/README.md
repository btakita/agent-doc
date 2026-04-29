# Agent Doc - VS Code Extension

Interactive document sessions with AI agents. Edit markdown documents in your editor while an AI agent responds in real-time via [agent-doc](https://github.com/btakita/agent-doc).

## Features

- **Submit documents** to Claude Code sessions via tmux routing
- **Claim documents** to assign them to specific Claude sessions
- **Sync tmux layout** to mirror your editor's tab arrangement
- **Highlight agent-doc markdown structures** including component comments, boundaries, prompts, `### Re:` headings, tracked ids, and scratch HTML comments
- **IPC patch watcher** applies agent responses directly via Document API (no external file change dialogs)
- **Component patching** with inline attribute support (`patch=append`, `patch=replace`; `mode=` accepted as backward-compatible alias)

## Requirements

- [agent-doc](https://github.com/btakita/agent-doc) CLI installed (`cargo install agent-doc`)
- [tmux](https://github.com/tmux/tmux) for session management
- [Claude Code](https://claude.ai/claude-code) for the AI agent backend

## Commands

| Command | Keybinding | Description |
|---------|-----------|-------------|
| Agent Doc: Run (Submit) | `Ctrl+Shift+Alt+A` | Route `/agent-doc` command to the correct Claude session |
| Agent Doc: Claim | `Ctrl+Shift+Alt+C` | Claim the current document for this tmux pane |
| Agent Doc: Sync Layout | `Ctrl+Shift+Alt+L` | Sync tmux panes to match editor tab layout |
| Agent Doc: Menu | `Alt+Enter` | Show action menu |

## How It Works

1. Open a markdown document with `agent_doc_session` frontmatter
2. Edit the document (your edits are the prompt)
3. Press `Ctrl+Shift+Alt+A` to submit
4. The agent responds and the response appears in your editor via IPC

The extension watches `.agent-doc/patches/` for JSON patch files from the CLI. When a patch arrives, it applies the changes via VS Code's Document API — preserving cursor position, undo history, and avoiding "externally modified" dialogs.

## Installation

**From Open VSX:**
Search "agent-doc" in VS Code/Cursor extensions.

**From CLI:**
```bash
agent-doc plugin install vscode   # VS Code
agent-doc plugin install cursor   # Cursor
```

**From GitHub Releases:**
Download `agent-doc-0.2.0.vsix` from [releases](https://github.com/btakita/agent-doc/releases) and install via Extensions > Install from VSIX.

## Links

- [agent-doc CLI](https://github.com/btakita/agent-doc)
- [crates.io](https://crates.io/crates/agent-doc)
- [PyPI](https://pypi.org/project/agent-doc/)
