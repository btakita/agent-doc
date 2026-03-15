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

## Module Layout

Use this layout when adding modules. Add new subcommands in their own file, wired through `main.rs`.

```
src/
  main.rs           # CLI entry point (clap derive)
  submit.rs         # Core loop: diff, send, merge-safe write, snapshot, git
  init.rs           # Scaffold session document
  reset.rs          # Clear session + snapshot
  diff.rs           # Preview diff (dry run) + comment stripping
  clean.rs          # Squash git history
  component.rs      # Component parser (<!-- agent:name --> markers) + name validation
  patch.rs          # Replace/append/prepend component content, config + shell hooks
  watch.rs          # Watch daemon: auto-submit on file change with debounce + loop prevention (reactive mode for stream docs)
  frontmatter.rs    # YAML frontmatter parse/write
  snapshot.rs       # Snapshot path/read/write
  git.rs            # Commit, branch, squash (includes `commit` subcommand)
  config.rs         # Global config (~/.config/agent-doc/config.toml)
  sessions.rs       # Session registry (sessions.json) + Tmux struct
  route.rs          # Route /agent-doc commands to correct tmux pane
  start.rs          # Start Claude session inside tmux pane
  claim.rs          # Claim document for current tmux pane
  focus.rs          # Focus tmux pane for a session document
  layout.rs         # Arrange tmux panes to mirror editor split layout
  outline.rs        # Markdown section structure + token counts
  prompt.rs         # Detect permission prompts from Claude Code sessions
  skill.rs          # Manage bundled SKILL.md (install/check)
  resync.rs         # Validate sessions.json, remove dead panes
  upgrade.rs        # Self-update via crates.io / GitHub Releases
  plugin.rs         # Editor plugin install/update/list via GitHub Releases
  crdt.rs           # CRDT foundation (yrs-based conflict-free merge)
  merge.rs          # 3-way merge + CRDT merge path
  stream.rs         # Stream command: real-time CRDT write-back loop
  agent/
    mod.rs          # Agent trait
    claude.rs       # Claude backend (Agent + StreamingAgent)
    streaming.rs    # StreamingAgent trait + stream-json parser
  audit_docs.rs     # Audit instruction files (via instruction-files crate)
editors/
  jetbrains/        # IntelliJ plugin (Kotlin/Gradle)
  vscode/           # VS Code extension (TypeScript)
```

## Release Process

1. Bump version in `Cargo.toml` + `pyproject.toml` (keep in sync)
2. `make check` (clippy + test)
3. Branch → PR → squash merge to main
4. Tag: `git tag v<version> && git push origin v<version>`
5. `cargo publish` (crates.io)
6. `maturin publish` (PyPI)
7. `gh release create v<version> --generate-notes` with prebuilt binary (GitHub Release)
8. Install binary: `cargo install --path .`

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

Stream flushes use **replace mode** for the target component regardless of the component's
configured mode. This is because the stream buffer is cumulative — each chunk contains the
full text so far, not just the delta. Without replace mode, append-mode components (like
`exchange`) would duplicate content on each flush.

Implementation: `flush_to_document()` passes mode overrides to
`template::apply_patches_with_overrides()`.

### IPC-First Writes (v0.17.5)

All write paths (`run`, `stream`, `write`) try IPC to the IDE plugin before falling back to direct disk writes. IPC writes a JSON patch file to `.agent-doc/patches/<hash>.json`; the IDE plugin applies it via Document API (preserving cursor, undo stack, no "externally modified" dialog) and deletes the file as ACK. If no plugin is active or ACK times out (2s), the write falls back to atomic disk write.

- `try_ipc()` — component-level patches for template/stream documents
- `try_ipc_full_content()` — full document replacement for append-mode documents
- Both are safe to call unconditionally; they return `false` immediately if `.agent-doc/patches/` does not exist

**Key files:** `crdt.rs` (CRDT foundation), `merge.rs` (CRDT merge path), `stream.rs` (command),
`agent/streaming.rs` (StreamingAgent trait + chunk parser), `agent/claude.rs` (streaming impl)

**Reactive file-watching:** CRDT-mode documents (`resolved.is_crdt()`) get reactive file-watching (zero debounce) from the watch daemon. The `WatchEntry` has a `reactive: bool` field set by `discover_entries()` for CRDT docs. Reactive paths are tracked in a `HashSet<PathBuf>` and use `Duration::ZERO` for the debounce check, enabling instant re-submit on file change.

**One session per document:** Each `agent-doc stream` spawns its own Claude CLI process.
Multiple documents stream in parallel via separate tmux panes.

**CRDT state storage:** `.agent-doc/crdt/<hash>.yrs` — persisted after each stream for
subsequent merges. Compacted via `agent-doc compact` to GC tombstones.

## Domain Ontology

agent-doc extends the existence kernel vocabulary (defined in `~/.claude/philosophy/src/`) with domain-specific terms. See the full ontology table in [README.md](README.md#domain-ontology).