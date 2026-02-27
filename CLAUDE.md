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

## Module Layout

Use this layout when adding modules. Add new subcommands in their own file, wired through `main.rs`.

```
src/
  main.rs           # CLI entry point (clap derive)
  submit.rs         # Core loop: diff, send, merge-safe write, snapshot, git
  init.rs           # Scaffold session document
  reset.rs          # Clear session + snapshot
  diff.rs           # Preview diff (dry run)
  clean.rs          # Squash git history
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
  prompt.rs         # Detect permission prompts from Claude Code sessions
  skill.rs          # Manage bundled SKILL.md (install/check)
  upgrade.rs        # Self-update via crates.io / GitHub Releases
  agent/
    mod.rs          # Agent trait
    claude.rs       # Claude backend
  audit_docs.rs     # Audit instruction files (via instruction-files crate)
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