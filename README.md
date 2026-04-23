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

The typical edit cycle: write in your editor, trigger `agent-doc route <file>` via a hotkey, and agent-doc injects the correct harness-specific trigger into the owning pane. Claude Code panes receive `/agent-doc <file>`; Codex panes receive plain `agent-doc <file>`.

## Key Features

- **Template mode** — named component regions (`<!-- agent:name -->`) updated independently; inline attrs (`patch=`, `max_lines=`) > `components.toml` > built-in defaults
- **CRDT merge** — yrs-based conflict-free merge for concurrent edits between agent writes and user edits
- **IPC-first writes** — socket IPC (Unix domain sockets); editor plugin receives JSON patches instead of file overwrites; preserves cursor position, undo history, and avoids "externally modified" dialogs
- **Tmux routing** — persistent per-document agent sessions; `route` dispatches to the correct pane or auto-starts one using the active harness's trigger shape (`/agent-doc` for Claude Code, plain `agent-doc` for Codex); reconciler always runs (no early exits) handling 0/1/2+ panes uniformly
- **Route readiness is binary-owned** — prompt detection / trigger acceptance lives in `route.rs` and must tolerate shell startup noise; the skill should not try to infer pane readiness from echoed command text
- **Harness-specific arg aliases** — `agent_args` is the generic override; `claude_args` and `codex_args` are harness-specific aliases used only by their matching backends
- **Streaming** — real-time CRDT write-back loop (`agent-doc stream`) with optional chain-of-thought routing
- **Parallel fan-out** — independent git worktrees per subtask, each with its own Claude session (`agent-doc parallel`)
- **Editor plugins** — JetBrains and VS Code plugins for hotkey integration and IPC writes
- **Watch daemon** — auto-submit on file change with debounce and reactive mode for stream documents
- **Linked resources** — `links` frontmatter field for local files and URLs; URL content fetched, converted HTML→markdown via `htmd`, cached, and diffed on each preflight
- **Session logging** — persistent logs at `.agent-doc/logs/<session-uuid>.log` for debugging session crashes and restarts
- **Preflight gate** — `agent-doc preflight` auto-recovers open `response_captured` / `write_applied` cycles, treats `staged snapshot already matches HEAD` as an already-committed no-op closeout instead of `commit_failed`, repairs stale `preflight_started` locks when the recorded snapshot/file hashes still match exactly, otherwise still requires a real pending/capture replay before auto-closing `preflight_started`, fails closed on unrecoverable `preflight_started` drift instead of snapshot-committing over newer live content, and emits `effective_tier`, `required_tier`, `suggested_tier`, `model_switch(_tier)`, and `agent_model` for the skill
- **Durable response capture** — every final response is persisted to `.agent-doc/captures/<doc-hash>/<cycle-id>.json` before write-back, so interrupted cycles can replay the exact response body instead of regenerating it
- **Template exchange guard** — template writes/compaction repair the safe case where a trailing `## User` / `## Assistant` / `### Re:` block escaped below `<!-- /agent:exchange -->`, but comment-only notes without escaped conversation headings stay outside; ambiguous mixed structure fails closed instead of being committed malformed
- **Respect manual cleanup** — if a user deletes that escaped conversation tail by hand in a template doc, `agent-doc repair` now discards the stale captured response and advances the snapshot to the repaired file instead of failing on a capture-baseline mismatch or reapplying the removed tail
- **Hard response commit boundary** — every appended response should cross a commit boundary unless the operator explicitly wants it left open; the normal happy path is `agent-doc finalize <file>`, while missed-patchback repair uses `agent-doc write --commit <file>`
- **Mode-aware bare/run entrypoint** — `agent-doc <file>` and `agent-doc run <file>` now share the same document-mode-aware path: template docs use template patchback, append docs use inline blocks, and missing format defaults to template
- **Binary-owned finalize path** — `agent-doc finalize <file>` is the strict happy-path response command: it reuses the normal write pipeline but requires the document to live in git and fails unless the cycle reaches a terminal committed state
- **Codex post-write guard** — the direct-exec Codex instruction path runs `agent-doc session-check <file>` after `finalize` or manual `write --commit`, and a nonzero check blocks success reporting until recovery finishes; it also flags likely direct response patchbacks (`### Re:` / `## Assistant`) that bypassed `agent-doc`, while self-healing already-committed historical response drift when `HEAD` proves the response is no longer out-of-band
- **Codex Stop-hook backstop** — `agent-doc skill install` for Codex now also writes `.codex/hooks.json` and enables `features.codex_hooks = true`, so `UserPromptSubmit` tracks the active document and `Stop` first tries to finish the response cycle deterministically from `last_assistant_message`; only if that auto-close path still fails does it fall back to capture-and-block / fail-closed behavior
- **Response persistence is the close-out boundary** — the instruction surface now requires requested implementation / verification / build-install work to finish before `finalize` or `write --commit`; after that point only `session-check`, recovery, and final reporting remain
- **Harness-owned full-suite verification** — agent harnesses must run the full project verification suite explicitly after code, test, build, or instruction-surface changes; `agent-doc` no longer installs a pre-commit hook that runs the whole suite implicitly
- **Model-attributed response headers** — `### Re:` headings should carry the resolved model short name (`gpt-5`, `opus-4-6`), not the harness label (`codex`, `claude`)
- **Git integration** — auto-commit each run; squash history with `agent-doc clean`
- **Commit self-heal** — `agent-doc commit` can absorb a narrowly-scoped missed agent patchback (`status`, `### Re:` response-block insertion, pending-ID superset) into the snapshot before staging, while still leaving plain user prompts uncommitted; already-committed historical response drift can repair the snapshot when the working tree matches `HEAD`, and a `HEAD`-current staged snapshot now closes the cycle as an explicit no-op instead of a false `commit_failed`
- **Extreme-drift guard stays conservative** — if a tracked document's snapshot is badly stale, `commit` warns but does not re-sync the snapshot from the live file wholesale; bootstrap scaffold auto-resync is limited to files with no `HEAD` entry yet so unanswered prompts cannot be swallowed into a commit
- **Clean post-commit cleanup** — commit strips transient `(HEAD)` markers from the staged snapshot so the committed blob stays clean, and post-commit cleanup now collapses snapshot/editor state back to the same clean boundary shape instead of leaving marker-only working-tree churn behind
- **Manual repair guidance is explicit across harnesses** — bundled skill/runbook content now distinguishes missing-user-prompt insertion from missed-response repair for both Claude Code and Codex: once the prompt is already present, the default documented repair path is `agent-doc write --commit <file>`, not direct file patching or a bare write-back that stops before commit
- **Generated harness instructions stay aligned** — installed Claude/Codex hot-path instructions are rendered from one shared source surface and parity-tested so completion-boundary guidance cannot silently drift between `.claude/skills/agent-doc/SKILL.md` and `.codex/AGENTS.md`
- **Bulk resync** — validates session state and fixes stale/orphaned panes in 2 subprocess calls instead of ~20-40; `--fix --session <name>` relocates WrongSession panes via join-pane instead of killing them
- **Column memory** — `.agent-doc/last_layout.json` remembers column→agent-doc mapping; preserves 2-pane tmux layout when one editor column switches to a non-agent file
- **Stash + rescue** — replaced panes are stashed (alive in background); stash rescue brings them back when the user switches to that document again
- **Startup lock** — `.agent-doc/starting/<hash>.lock` with 5s TTL prevents double-spawn when sync fires twice in quick succession
- **Component-aware baseline guard** — detects stale baselines by comparing append-mode components only; user edits to replace-mode components (status, pending) don't trigger false positives
- **Hook system** — cross-session event coordination via `agent-doc hook fire/poll/listen/gc`; `post_write` / `post_commit` events now include `capture_id` and `response_sha256` when a durable capture exists, the system integrates with Claude Code hooks via `PostToolUse`, and Codex installs a repo-local `UserPromptSubmit` / `Stop` bridge through `.codex/hooks.json`
- **Slash command dispatch** — `preflight` extracts slash commands from user-added diff lines (`parse_slash_commands`); the SKILL executes them before responding; guards exclude code fences, blockquotes, and non-added lines
- **Dedupe stale patch cleanup** — after removing duplicate blocks, `dedupe` also deletes the stale `.agent-doc/patches/<hash>.json` to prevent the plugin's startup scan from re-applying removed content

## Architecture

The binary owns all deterministic behavior: component parsing, patch application, CRDT merge, snapshot management, git operations, tmux routing, and IPC writes. The bundled skill / AGENTS instructions are the non-deterministic orchestrator layer — they read the diff, generate responses, and decide what to write.

**Binary vs. Agent Responsibility:**

| Responsibility | Owner | Why |
|---------------|-------|-----|
| Component parsing, patch application, mode resolution | **Binary** (Rust) | Deterministic, testable, consistent across agents |
| CRDT merge, snapshot management, atomic writes | **Binary** (Rust) | Concurrency safety requires flock + atomic rename |
| Diff computation, comment stripping, truncation detection | **Binary** (Rust) | Reproducible baseline comparison |
| Git operations (commit, history, clean) | **Binary** (Rust) | Direct `std::process::Command` calls; selective commit can self-heal narrow missed agent-owned drift and normalize boundary cleanup without absorbing free-form user prompts |
| Tmux routing, session registry, pane management | **Binary** (Rust) | Process-level coordination |
| Pre-response snapshots, undo, extract, transfer | **Binary** (Rust) | File-level atomicity |
| Boundary marker lifecycle (insert, reposition, cleanup) | **Binary** (Rust) | Deterministic, all write paths need it |
| Reading diff, interpreting user intent | **Skill** (SKILL.md) | Requires LLM reasoning |
| Generating response content | **Skill** (SKILL.md) | Non-deterministic |
| Deciding what to write to which component | **Skill** (SKILL.md) | Context-dependent, including the shared Claude/Codex manual-repair rule to use `agent-doc write --commit <file>` when the prompt already exists |
| Enforcing response-cycle completion for the normal happy path | **Binary** (Rust) | `agent-doc finalize` fails closed unless the write reaches a terminal committed cycle state |
| Streaming checkpoints, progress tracking | **Skill** (SKILL.md) | Response-generation timing |
| Pending item management (parse, populate, process) | **Skill** (SKILL.md) | Semantic understanding of prompts |

See [CLAUDE.md](CLAUDE.md) for the full module layout, stream mode details, and release process.

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
| **Provisioning** | Creating a new tmux pane and starting the configured agent harness for a document. Performed by `route::auto_start`. The normal path for new documents — sync triggers provisioning when it finds a session UUID with no registered pane. |
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
| **Cycle** | One round-trip: user edits -> preflight -> agent response -> write-back -> commit. Logged in `.agent-doc/logs/cycles.jsonl` with git state references for reproducibility, with the current per-document phase tracked in `.agent-doc/state/cycles/<doc-hash>.json` and the exact response body durably captured in `.agent-doc/captures/<doc-hash>/<cycle-id>.json` so interrupted cycles can be replayed or blocked exactly. |
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
