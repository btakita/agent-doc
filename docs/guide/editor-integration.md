# Editor Integration

agent-doc is designed to be triggered from your editor with a single hotkey.

Both official editor plugins also add lightweight visual distinction for agent-doc structures in markdown: component comments, patch comments, boundary markers, `### Re:` headings, `❯` prompts, tracked `[#id]` tags, and plain HTML scratch comments plus their bodies are highlighted directly in the editor, while fenced code examples are left alone. JetBrains component bodies and markdown emphasis inherit the editor's normal text color; underscores inside identifier words such as `foo_bar_baz` are not treated as emphasis delimiters. The JetBrains plugin reapplies those visual tokens when markdown files are opened or selected cold from disk, before the document has been edited in the IDE buffer.

## Installing editor plugins

This section moved here from the project [README](../../README.md).

### JetBrains (IntelliJ IDEA, Rider, PyCharm, and other IntelliJ-platform IDEs)

```sh
agent-doc plugin install jetbrains
```

When multiple JetBrains IDE data roots exist, select the target explicitly for automation:

```sh
agent-doc plugin install jetbrains --plugins-dir ~/.local/share/JetBrains/IdeaIC2026.1/plugins
```

The target must remain under a versioned JetBrains IDE data root so agent-doc can
prove its platform build. Builds 242–261 receive the classic package; build 262
receives the separately ranged modular package. An unprovable or unsupported
target is refused instead of guessing an artifact.

Without the CLI, add the plugin's custom repository in the IDE instead:
**Settings → Plugins → ⚙ → Manage Plugin Repositories**, add the listing that
matches your IDE build from
[JetBrains plugin distribution](../reference/jetbrains-distribution.md#production-repository-urls),
then install **Agent Doc** from the Marketplace tab. Releases are also uploaded
to JetBrains Marketplace; if a search there does not find **Agent Doc** yet,
use the CLI or the custom repository.

If `agent-doc plugin install jetbrains` reports that no plugins directory was
found, start the IDE once so it creates its data directory, then retry.

Restart the IDE if it asks you to. The plugin loads agent-doc's native library
(`agent-doc lib-path` prints where it is); the shell installer, PyPI, and
Homebrew installs put it beside the binary.

### VS Code

```sh
agent-doc plugin install vscode
```

The command downloads the `.vsix` from the matching GitHub Release, verifies
it, and installs it with the first of `cursor`, `codium`, or `code` found on
`PATH`. To install by hand, download `agent-doc-<version>.vsix` from
[GitHub Releases](https://github.com/btakita/agent-doc/releases) and run
**Extensions: Install from VSIX...** in the editor. The extension is not
published to the VS Code Marketplace.

### Zed

Install `editors/zed` as a Zed dev extension. It starts the supplemental
`agent-doc zed-lsp` server alongside Zed's normal Markdown support. Only buffers
with valid, non-empty `agent_doc_session` frontmatter enter agent-doc mode;
ordinary Markdown buffers are a strict no-op. See
[`editors/zed/README.md`](../../editors/zed/README.md).

For how editor hotkeys reach tmux panes, see [Tmux Routing and Navigation Lanes](tmux-routing.md).

## Hotkey-only setup (no plugin)

### JetBrains (IntelliJ, WebStorm, etc.)

**Settings > Tools > External Tools > Add:**

| Field | Value |
|-------|-------|
| Program | `agent-doc` |
| Arguments | `run $FilePath$` |
| Working directory | `$ProjectFileDir$` |

Assign a keyboard shortcut (e.g. `Ctrl+Shift+S`). The External Tool shows output in the Run panel — progress messages, merge status, and errors all appear there.

### VS Code task

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

### Vim / Neovim

```vim
nnoremap <leader>as :!agent-doc run %<CR>:e<CR>
```

The `:e<CR>` reloads the file after the response is written.

## General tips

- **Don't edit during run** — the merge-safe flow handles it, but it's simpler to wait for the progress indicator to finish.
- **Auto-reload** — JetBrains and VS Code auto-reload files changed on disk. Vim needs the `:e` reload.
- **Diff gutters** — agent-doc commits your edits before each turn, so while a turn is in progress the agent's new text shows as a diff against that commit. The turn's closeout then commits the response too; use `agent-doc log <file>` or `git log` to review past turns.
