# agent-doc

Interactive document sessions with AI agents.

## Conventions

- Use `clap` derive for CLI argument parsing
- Use `serde` derive for all data types
- Use `serde_yaml` for frontmatter parsing
- Use `similar` crate for diffing (pure Rust, no shell `diff` dependency)
- Use `serde_json` for agent response parsing
- Use `std::process::Command` for git operations (not `git2`)
- Use `toml + serde` for config file parsing
- No async — sequential per-run
- Use `anyhow` for application errors
- **NEVER swallow errors** — no `let _ =` on fallible operations. Always log at minimum a warning to stderr. Silent failures make bugs invisible and waste debugging cycles.
- **All deterministic behavior in the binary** — document manipulation (compact, diff, merge, patch, write), snapshot management, git operations, and component parsing must live in Rust. The SKILL.md skill is the non-deterministic orchestrator (reads diff, generates response, decides what to write). Never implement deterministic document logic in the skill or ad-hoc scripts.
- **Harness arg resolution is explicit** — `agent_args` is the shared override, `claude_args` is Claude-only, and `codex_args` is Codex-only. Keep those precedence chains in `start.rs`, `frontmatter.rs`, `config.rs`, and the docs/specs aligned.
- **Selective commit is conservative** — `git::commit()` stages the snapshot, not arbitrary working-tree drift. The only absorbable out-of-band repair path is narrow agent-owned drift (`status`, appended `### Re:` response, pending-ID superset) when the redacted component structure still matches. Plain user prompts must remain uncommitted. Even under extreme snapshot/file drift, tracked docs must not wholesale re-sync the snapshot from the live file — reserve that bootstrap escape hatch for untracked scaffold snapshots only. After a successful commit, boundary cleanup must keep the committed blob clean while preserving a single visible ` (HEAD)` marker on the current response heading in the snapshot / user-facing document state; only skip the direct file rewrite when a live IPC listener is available to perform that visible rewrite in the editor.
- **FFI-first for editor integration (Shared Foundation pattern)** — when adding features that editors need (sync debounce, busy guards, IPC listeners, layout validation), implement in the FFI layer (`ffi.rs`) first, then call from editor plugins via JNA/FFI. Editor plugins should be thin event reporters — layout changed, file selected, etc. Business logic (debouncing, locking, socket listeners, idempotency checks) belongs in the shared FFI library, not duplicated across IntelliJ/VS Code plugins. **Ontology:** Both the FFI library and each editor plugin are **Systems** with their own **Perspectives**. Each exposes an **Interface** (C ABI, JNA bindings) — the defined boundary through which Systems communicate. The Shared Foundation pattern places shared logic at the broadest **Scope** (FFI library) so all consumer Systems access it through their Interfaces. **Test:** "Does this feature need to work in >1 editor?" → implement in FFI. Example: socket IPC listener lives in `ffi.rs` (`agent_doc_start_ipc_listener`), not in `PatchWatcher.kt`.

## Binary vs Agent Responsibility

See [README.md](README.md) for the full responsibility table.

**Rule of thumb:** If the operation can be unit-tested with fixed inputs → binary. If it requires understanding natural language → skill.
- **Inline component attributes:** `<!-- agent:name patch=append max_lines=50 -->` — patch mode and max_lines are configurable on the tag itself. `mode=` is accepted as a backward-compatible alias for `patch=`; `patch=` takes precedence if both are present. `max_lines=N` trims content to the last N lines after patching (0 or absent = unlimited). Precedence: inline attr > `components.toml` > built-in defaults.
- **`agent_doc_format: inline`** is the canonical name for the old "append" format (`append` accepted as backward-compat alias). Template mode uses components; inline mode uses User/Assistant blocks.

## Module Layout

Use this layout when adding modules. Add new subcommands in their own file, wired through `main.rs`.

```
src/
  main.rs           # CLI entry point (clap derive)
  submit.rs         # Core loop: diff, send, merge-safe write, snapshot, git
  init.rs           # Scaffold session document; no-arg mode initializes project (.agent-doc/ dirs + SKILL.md)
  reset.rs          # Clear session + snapshot
  dedupe.rs         # Remove consecutive duplicate response blocks
  diff.rs           # Preview diff (dry run) + comment stripping
  clean.rs          # Squash git history
  component.rs      # Component parser (<!-- agent:name --> markers) + name validation
  patch.rs          # Replace/append/prepend component content, config + shell hooks
  watch.rs          # Watch daemon: auto-submit on file change with debounce + loop prevention (reactive mode for stream docs)
  frontmatter.rs    # YAML frontmatter parse/write
  snapshot.rs       # Snapshot path/read/write
  git.rs            # Commit, branch, squash (includes `commit` subcommand + narrow missed-patchback self-heal)
  config.rs         # Global config (~/.config/agent-doc/config.toml)
  sessions.rs       # Session registry (sessions.json) + Tmux struct
  route.rs          # Route harness-specific agent-doc triggers to the correct tmux pane (pub auto_start for sync.rs)
  start.rs          # Start configured agent harness inside tmux pane
  claim.rs          # Claim document for current tmux pane
  focus.rs          # Focus tmux pane for a session document
  layout.rs         # Arrange tmux panes to mirror editor split layout
  outline.rs        # Markdown section structure + token counts
  prompt.rs         # Detect permission prompts from Claude Code sessions (strip_ansi is pub(crate))
  skill.rs          # Manage bundled SKILL.md (install/check)
  install.rs        # System-level setup: check prerequisites (tmux + agent CLI) and install editor plugins
  resync.rs         # Validate sessions.json, remove dead panes, detect wrong-session/wrong-process panes (--fix [--session <target>])
  session_cmd.rs    # Show/set configured tmux session with pane migration
  history.rs        # Exchange version history from git + restore
  upgrade.rs        # Self-update via crates.io / GitHub Releases
  plugin.rs         # Editor plugin install/update/list via GitHub Releases
  write.rs          # Write command: parse patches, IPC-first writes, disk fallback
  template.rs       # Template mode: patch parsing, apply_patches, boundary lifecycle
  boundary.rs       # Boundary marker management (insert, remove, reposition)
  crdt.rs           # CRDT foundation (yrs-based conflict-free merge)
  merge.rs          # 3-way merge + CRDT merge path
  stream.rs         # Stream command: real-time CRDT write-back loop
  ffi.rs            # C ABI exports for editor plugins (JNA/FFI); ffi_git_commit unsets GIT_DIR/GIT_INDEX_FILE/GIT_WORK_TREE so it works correctly when called from git hook contexts
  ipc_socket.rs     # Socket-based IPC (Unix domain sockets via interprocess crate)
  lib.rs            # Library target re-exports
  recover.rs        # Orphaned pending response detection + recovery
  compact.rs        # Exchange compaction (archive + truncate)
  convert.rs        # Bidirectional format conversion (inline ↔ template)
  extract.rs        # Extract exchange sections to new documents
  undo.rs           # Undo last response (pre-response snapshot restore)
  mode.rs           # Document mode resolution (format + write strategy)
  autoclaim.rs      # SessionStart hook: auto-claim documents
  commands.rs       # List available commands for plugin autocomplete
  hooks.rs          # Cross-session hook integration (fire_post_write, fire_post_commit, etc.)
  hook_cmd.rs       # CLI subcommands: agent-doc hook fire/poll/listen/gc
  ops_log.rs        # Best-effort operational logging to .agent-doc/logs/ops.log
  cycle_state.rs    # Persisted per-document cycle phase/hash state for interrupted-cycle enforcement
  sync.rs           # Sync pane state between editor and tmux (reconciler always runs, no early exits, column memory)
  preflight.rs      # Pre-agent checks: layout check, recover, commit, claims, diff, document read → JSON
  model_tier.rs     # Harness-agnostic model tier selection (Tier enum, config, heuristic, scanner, composition)
  agent/
    mod.rs          # Agent trait
    claude.rs       # Claude backend (Agent + StreamingAgent)
    junie.rs        # Junie backend (Agent + StreamingAgent)
    streaming.rs    # StreamingAgent trait + stream-json parser
  terminal.rs       # Launch external terminal with tmux session
  parallel.rs       # Parallel fan-out with git worktrees
  worktree.rs       # Git worktree management for parallel sessions
  audit_docs.rs     # Audit instruction files (via instruction-files crate)
editors/
  jetbrains/        # IntelliJ plugin (Kotlin/Gradle)
  vscode/           # VS Code extension (TypeScript)
```

## Release Process

1. Bump version in `Cargo.toml` + `pyproject.toml` (keep in sync)
2. Update `VERSIONS.md` with a new version entry summarizing the changes
3. `make check` (clippy + test)
4. `cargo install --path .` — install locally and verify the changed behavior end-to-end
5. **Manual testing gate:** Do NOT proceed to step 6+ until the user has confirmed the feature works in a live session
6. Branch → PR → squash merge to main
7. Tag: `git tag v<version> && git push origin v<version>`
8. `cargo publish` (crates.io)
9. `maturin publish` (PyPI)
10. `gh release create v<version> --generate-notes` with prebuilt binary (GitHub Release)

## Agent Backend Contract

Each agent backend implements: take a prompt string, return (response_text, session_id).
The prompt includes the diff and full document. The agent backend handles CLI
invocation, JSON parsing, and session flags.

### StreamingAgent Contract

Streaming backends implement `StreamingAgent::send_streaming()` → `Iterator<StreamChunk>`.
Used by `agent-doc stream` for real-time write-back. Currently only `claude` supports streaming
(via `--output-format stream-json`). Each `StreamChunk` has cumulative text, optional
`thinking` content, `is_final` flag, and optional `session_id` on the final chunk.

## Stream Mode

Stream mode (`agent_doc_format: template` + `agent_doc_write: crdt`) enables real-time agent output with CRDT-based conflict-free merge. Legacy: `agent_doc_mode: stream` is still supported as a deprecated alias.

**Usage:** `agent-doc stream <FILE> [--interval 200] [--agent claude] [--model opus] [--no-git]`

**How it works:**
1. Validates document uses CRDT write strategy (`resolved.is_crdt()`), reads `StreamConfig` from frontmatter
2. Computes diff, builds prompt requesting patch-block format
3. Spawns streaming agent (`claude -p --output-format stream-json`)
4. Timer thread (default 200ms) periodically flushes accumulated text to document:
   `flock → read file → apply template patch (replace mode) → atomic write → unlock`
5. On completion: saves CRDT state + snapshot, updates resume ID, optional git commit

**Frontmatter:**
```yaml
agent_doc_format: template
agent_doc_write: crdt
agent_doc_stream:
  interval: 200           # write-back interval (ms), default 200
  strip_ansi: true        # strip ANSI codes from output
  target: exchange        # target component name
  thinking: false         # include chain-of-thought (default: false)
  thinking_target: log    # route thinking to separate component (optional)
```

### Chain of Thought

Stream mode can capture the agent's chain-of-thought (thinking blocks) from Claude's
`stream-json` output. Controlled by `thinking` and `thinking_target` in `StreamConfig`:

| Config | Behavior |
|--------|----------|
| `thinking: false` (default) | Thinking blocks silently skipped |
| `thinking: true` (no `thinking_target`) | Thinking interleaved in target component as `<details><summary>Thinking</summary>...</details>` |
| `thinking: true` + `thinking_target: log` | Thinking routed to separate `<!-- agent:log -->` component; response goes to target |

**Parser:** `extract_assistant_content()` in `streaming.rs` extracts both `"type": "text"` and
`"type": "thinking"` content blocks. Thinking is buffered separately (`thinking_buffer`) and
flushed on the same timer interval as response text.

Use this frontmatter structure to enable thinking with a separate log component:
```markdown
---
agent_doc_format: template
agent_doc_write: crdt
agent_doc_stream:
  target: exchange
  thinking: true
  thinking_target: log
---
<!-- agent:exchange -->
User prompt here.
<!-- /agent:exchange -->
<!-- agent:log -->
<!-- /agent:log -->
```

### Flush Behavior

Stream flushes use the normal mode resolution chain (inline attr > `components.toml` > built-in default). `run_stream()` no longer hardcodes replace mode for exchange — mode resolution applies normally.

Implementation: `flush_to_document()` uses `template::apply_patches()`.

### IPC-First Writes (v0.17.5)

All write paths (`run`, `stream`, `write`) try IPC to the IDE plugin when `.agent-doc/patches/` exists (plugin installed) and `--force-disk` is not set. IPC writes a JSON patch file to `.agent-doc/patches/<hash>.json`; the IDE plugin applies it via Document API (preserving cursor, undo stack, no "externally modified" dialog) and deletes the file as ACK. On IPC timeout (2s), exits with code 75 (`EX_TEMPFAIL`) instead of falling back to disk write. Use `agent-doc write --force-disk` to bypass IPC and write directly to disk.

- `try_ipc(file, patches, unmatched, frontmatter_yaml, baseline, content_ours)` — component-level patches for template/stream documents. `content_ours` (baseline + response, no user edits) is saved as the snapshot on IPC success; pass `None` to fall back to reading the current file.
- `try_ipc_full_content()` — full document replacement for inline-mode documents
- Both are safe to call unconditionally; they return `false` immediately if `.agent-doc/patches/` does not exist
- When no explicit patches exist but unmatched content targets `exchange`/`output` and a boundary marker is present, `try_ipc()` synthesizes a boundary-aware exchange patch automatically

**Key files:** `crdt.rs` (CRDT foundation), `merge.rs` (CRDT merge path), `stream.rs` (command),
`agent/streaming.rs` (StreamingAgent trait + chunk parser), `agent/claude.rs` (streaming impl)

**Reactive file-watching:** CRDT-mode documents (`resolved.is_crdt()`) get reactive file-watching (zero debounce) from the watch daemon. The `WatchEntry` has a `reactive: bool` field set by `discover_entries()` for CRDT docs. Reactive paths are tracked in a `HashSet<PathBuf>` and use `Duration::ZERO` for the debounce check, enabling instant re-submit on file change.

**One session per document:** Each `agent-doc stream` spawns its own Claude CLI process.
Multiple documents stream in parallel via separate tmux panes.

**CRDT state storage:** `.agent-doc/crdt/<hash>.yrs` — persisted after each stream for
subsequent merges. Compacted via `agent-doc compact` to GC tombstones.

## Domain Ontology

agent-doc extends the existence kernel vocabulary with domain-specific terms. See the full ontology table in [README.md](README.md#domain-ontology).
