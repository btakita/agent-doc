# agent-doc

Interactive document sessions with AI agents.

Edit a markdown file, press a hotkey, and the tool diffs your changes, sends them to an AI agent, and writes the response back into the document. The document is the UI.

> **Alpha Software** — actively developed; APIs and frontmatter format may change between versions.

> **Single-user only.** agent-doc operates on the local filesystem with no access control. Use a private git repository. See the [Security](#security) section for details.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/btakita/agent-doc/main/install.sh | sh
```

**Alternatives:**

```sh
# From crates.io
cargo install agent-doc

# From PyPI
pip install agent-doc

# From source
cargo build --release
cargo install --path .
```

## Quick Start

```sh
# 1. Initialize project (creates .agent-doc/ and installs SKILL.md)
agent-doc init

# 2. Scaffold a session document
agent-doc init session.md "My Topic"

# 3. Claim the document to the current tmux pane
agent-doc claim session.md

# 4. Route hotkey triggers to the correct tmux pane
agent-doc route session.md

# 5. Run: diff, send to agent, write response back
agent-doc run session.md
```

The typical edit cycle: write in your editor, trigger `agent-doc route <file>` via a hotkey, the agent responds in the same document.

## Key Features

- **Template mode** — named component regions (`<!-- agent:name -->`) updated independently; inline attrs (`patch=`, `max_lines=`) > `components.toml` > built-in defaults
- **CRDT merge** — yrs-based conflict-free merge for concurrent edits between agent writes and user edits
- **IPC-first writes** — socket IPC (Unix domain sockets); editor plugin receives JSON patches instead of file overwrites; preserves cursor position, undo history, and avoids "externally modified" dialogs
- **Tmux routing** — persistent Claude Code sessions per document; `route` dispatches to the correct pane or auto-starts one; reconciler always runs (no early exits) handling 0/1/2+ panes uniformly
- **Streaming** — real-time CRDT write-back loop (`agent-doc stream`) with optional chain-of-thought routing
- **Parallel fan-out** — independent git worktrees per subtask, each with its own Claude session (`agent-doc parallel`)
- **Editor plugins** — JetBrains and VS Code plugins for hotkey integration and IPC writes
- **Watch daemon** — auto-submit on file change with debounce and reactive mode for stream documents
- **Linked resources** — `links` frontmatter field for local files and URLs; URL content fetched, converted HTML→markdown via `htmd`, cached, and diffed on each preflight
- **Session logging** — persistent logs at `.agent-doc/logs/<session-uuid>.log` for debugging session crashes and restarts
- **Git integration** — auto-commit each run; squash history with `agent-doc clean`
- **Bulk resync** — validates session state and fixes stale/orphaned panes in 2 subprocess calls instead of ~20-40
- **Column memory** — `.agent-doc/last_layout.json` remembers column→agent-doc mapping; preserves 2-pane tmux layout when one editor column switches to a non-agent file
- **Stash + rescue** — replaced panes are stashed (alive in background); stash rescue brings them back when the user switches to that document again
- **Startup lock** — `.agent-doc/starting/<hash>.lock` with 5s TTL prevents double-spawn when sync fires twice in quick succession
- **Component-aware baseline guard** — detects stale baselines by comparing append-mode components only; user edits to replace-mode components (status, pending) don't trigger false positives
- **Hook system** — cross-session event coordination via `agent-doc hook fire/poll/listen/gc`; integrates with Claude Code hooks via `PostToolUse` bridge
- **Slash command dispatch** — `preflight` extracts slash commands from user-added diff lines (`parse_slash_commands`); the SKILL executes them before responding; guards exclude code fences, blockquotes, and non-added lines
- **Dedupe stale patch cleanup** — after removing duplicate blocks, `dedupe` also deletes the stale `.agent-doc/patches/<hash>.json` to prevent the plugin's startup scan from re-applying removed content

## Architecture

The binary owns all deterministic behavior: component parsing, patch application, CRDT merge, snapshot management, git operations, tmux routing, and IPC writes. The SKILL.md Claude Code skill is the non-deterministic orchestrator — it reads the diff, generates responses, and decides what to write.

See [CLAUDE.md](CLAUDE.md) for the full module layout, binary vs. agent responsibility table, stream mode details, and release process.

## Supported Editors

**JetBrains (IntelliJ, PyCharm, etc.)**

```sh
agent-doc plugin install jetbrains
```

Or install from JetBrains Marketplace. Configure an External Tool: Program=`agent-doc`, Args=`run $FilePath$`, Working dir=`$ProjectFileDir$`. Assign a keyboard shortcut.

**VS Code**

```sh
agent-doc plugin install vscode
```

Or install from the VS Code Marketplace. Add a task with `"command": "agent-doc run ${file}"` and bind it to a keybinding.

**Vim/Neovim**

```vim
nnoremap <leader>as :!agent-doc run %<CR>:e<CR>
```

## Domain Ontology

agent-doc extends the [existence kernel vocabulary](https://github.com/btakita/existence-lang) with domain-specific terms.

### Document Lifecycle

| Term | Definition |
|------|-----------|
| **Session** | A persistent conversation between a user and an agent, identified by UUID. Stored in frontmatter as `agent_doc_session`. |
| **Document** | A markdown file that serves as the UI for a session. Contains frontmatter, components, and user/agent content. |
| **Snapshot** | A baseline copy of the document at a known state. Used for diff computation and CRDT merge. |
| **Component** | A named region in a template document (`<!-- agent:name -->...<!-- /agent:name -->`). Targeted by patch blocks. |
| **Boundary** | A marker (`<!-- agent:boundary:hash -->`) that separates committed content from uncommitted user edits. |
| **Exchange** | The shared conversation surface where user and agent write inline. A component with `patch=append`. |

### Pane Lifecycle

| Term | Definition |
|------|-----------|
| **Binding** | The document→pane association stored in `sessions.json`. Created by `claim` (explicit) or `auto_start` (automatic). One document per pane. |
| **Reconciliation** | The process of matching editor layout to tmux layout. Performed by `sync`. Stashes unwanted panes, provisions missing ones. |
| **Provisioning** | Creating a new tmux pane and starting a Claude session for a document. Performed by `route::auto_start`. The normal path for new documents — sync triggers provisioning when it finds a session UUID with no registered pane. |
| **Initialization** | Assigning a session UUID, creating a snapshot, and committing to git. Performed by `ensure_initialized()`. Called from claim, preflight, and sync's resolve_file. |

### Integration Layer

| Term | Definition |
|------|-----------|
| **Route** | Resolve which tmux pane handles a file. Creates panes if needed (provisioning). |
| **Sync** | Reconcile editor layout with tmux layout. The primary entrypoint from the JB plugin on every tab switch. |
| **Claim** | Bind a document to a specific existing pane. Used for manual pane assignment; not needed in normal editor workflow (sync + auto_start handles it). |

### Interaction Model

| Term | Definition |
|------|-----------|
| **Directive** | A signal that authorizes and requests action. User inputs like "do", "go", "yes" are directives. Classified as `DiffType::Approval` in preflight. The directive's brevity is independent of the expected execution thoroughness — quality processes always apply in full. |
| **Cycle** | One round-trip: user edits -> preflight -> agent response -> write-back -> commit. Logged in `.agent-doc/logs/cycles.jsonl` with git state references for reproducibility. |
| **Layout check** | Pre-agent tmux health inspection (`check_layout()`). Detects: missing window 0, non-idle stash panes, and session drift (registered panes spanning multiple tmux sessions). Reported as `layout_issues[]` in preflight JSON. |
| **Session drift** | Condition where registered document panes span more than one tmux session. Detected by preflight's `check_layout()`. Fixed by `agent-doc session set <N>` to consolidate panes into the target session. |
| **Diff** | The user's changes since the last snapshot. Classified by `classify_diff()` into a `DiffType` for skill routing. Comment-stripped before comparison. |
| **Annotation** | A user edit to agent-written content (inline modification, colon-append). Classified as `DiffType::Annotation`. |

## Security

agent-doc is designed for **single-user, local operation**. All session data (documents, snapshots, exchange history) is stored on the local filesystem and committed to a git repository.

**Current security model:**
- **Single user only.** There is no multi-user access control, authentication, or session isolation.
- **Private repo recommended.** Session documents may contain sensitive content (correspondence, research, credentials in context). Use a private git repository.
- **Prompt injection risk.** Content pasted into documents from external sources (emails, web pages, chat logs) could contain prompt injection attempts. The agent processes all document content as user input with no injection scanning.
- **`--dangerously-skip-permissions` exposure.** When running with this flag (common in agent-doc sessions), the agent has full filesystem access. Injected prompts could read files or execute commands if not sandboxed.

**Planned:** Collaborative security for web/networked deployments (multi-user access control, session isolation, content scanning, compartmented access patterns).

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option.
