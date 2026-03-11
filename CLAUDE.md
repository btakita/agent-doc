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
  watch.rs          # Watch daemon: auto-submit on file change with debounce + loop prevention
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
  agent/
    mod.rs          # Agent trait
    claude.rs       # Claude backend
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

## Domain Ontology

agent-doc extends the existence kernel vocabulary (defined in `~/.claude/philosophy/src/`) with domain-specific terms for interactive document sessions. These terms map agent-doc concepts to the universal ontology they derive from.

| Term | Derives From | Description |
|------|-------------|-------------|
| **Session** | project + story | A bounded interaction with temporal arc; the unit of agent-doc work |
| **Document** | entity + context | A markdown file that holds conversational state between user and agent |
| **Pane** | focus + scope | A tmux viewport — finite attention applied to a single document |
| **Claim** | scope + entity | Binding a document to a pane; scoping focus to a specific file |
| **Route** | context + resolution | Resolving which pane handles a document; context-aware dispatch |
| **Sync** | pattern + system | Aligning tmux pane layout to editor split state; maintaining coherence |
| **Watch** | consciousness + evolution | Detecting file changes and triggering agent responses; event-driven |
| **Dashboard** | system + focus | A document used as a live system view with agent-maintained sections |
| **Component** | scope + abstraction | Bounded, named, re-renderable region in a document (`<!-- agent:name -->...<!-- /agent:name -->`). Configurable mode (replace/append/prepend) and shell hooks. |
| **Registry** | system + perspective | Persistent mapping of documents to panes; the routing state |
| **Snapshot** | entity + story | Point-in-time capture of document content for diff computation |
| **Project** | system + scope | The bounded working context; identified by `.agent-doc/` at its root. Contains documents, registry, snapshots, daemon. tmux-router is project-agnostic. |
| **Overlay** | context + resolution | Domain-specific terms extending the base kernel vocabulary |