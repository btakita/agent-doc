# Editor Integration

agent-doc is designed to be triggered from your editor with a single hotkey.

## JetBrains (IntelliJ, WebStorm, etc.)

**Settings > Tools > External Tools > Add:**

| Field | Value |
|-------|-------|
| Program | `agent-doc` |
| Arguments | `run $FilePath$` |
| Working directory | `$ProjectFileDir$` |

Assign a keyboard shortcut (e.g. `Ctrl+Shift+S`). The External Tool shows output in the Run panel — progress messages, merge status, and errors all appear there.

## VS Code

Add a task to `.vscode/tasks.json`:

```json
{
    "label": "agent-doc run",
    "type": "shell",
    "command": "agent-doc run ${file}",
    "group": "build",
    "presentation": {
        "reveal": "silent",
        "panel": "shared"
    }
}
```

Bind to a keybinding in `keybindings.json`:

```json
{
    "key": "ctrl+shift+s",
    "command": "workbench.action.tasks.runTask",
    "args": "agent-doc run"
}
```

## Vim / Neovim

```vim
nnoremap <leader>as :!agent-doc run %<CR>:e<CR>
```

The `:e<CR>` reloads the file after the response is written.

## General tips

- **Don't edit during run** — the merge-safe flow handles it, but it's simpler to wait for the progress indicator to finish.
- **Auto-reload** — JetBrains and VS Code auto-reload files changed on disk. Vim needs the `:e` reload.
- **Diff gutters** — after run, your editor shows diff gutters for everything the agent added (because agent responses are left uncommitted).
