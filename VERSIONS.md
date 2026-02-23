# Versions

agent-doc is alpha software. Expect breaking changes between minor versions.

Use `BREAKING CHANGE:` prefix in version entries to flag incompatible changes.

## 0.1.0

Initial release.

- **Interactive document sessions**: Edit a markdown document, submit to an AI agent, response appended back into the document.
- **Session continuity**: YAML frontmatter tracks session ID, agent backend, and model. Fork from current session on first submit, resume on subsequent.
- **Diff-based submits**: Only changed content is sent as a diff, with the full document for context. Double-submit guard via snapshots.
- **Merge-safe writes**: 3-way merge via `git merge-file` if the file is edited during agent response. Conflict markers written on merge failure.
- **Git integration**: Pre-commit user changes before agent call, leave agent response uncommitted for editor diff gutters. `-b` flag for auto-branch, `--no-git` to skip.
- **Agent backends**: Agent-agnostic core. Claude backend included. Custom backends configurable via `~/.config/agent-doc/config.toml`.
- **Commands**: `submit`, `init`, `diff`, `reset`, `clean`, `audit-docs`.
- **Editor integration**: JetBrains External Tool, VS Code task, Vim/Neovim mapping.
