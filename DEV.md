---
project_type: rust-maturin
publication_targets:
  - crates.io
  - pypi
  - github-release
secret_paths:
  crates_io: "btak/CARGO_REGISTRY_TOKEN"
  pypi: "pypi/token"
post_release:
  - "cargo install --path ."
  - "cargo build --release"
---

# agent-doc — Release Notes

## Post-release: cargo install is critical

After publishing, always run `cargo install --path .` — the `~/.cargo/bin/agent-doc`
binary is used by tmux-spawned sessions. The `.bin/agent-doc` symlink (from `make release`)
serves the local workspace but tmux panes resolve via `$PATH` which hits `~/.cargo/bin/`.

## Skill update after release

After publishing a new version, run `agent-doc skill install --reload restart` inside
the agent-loop workspace to update the Claude Code skill (`.claude/skills/agent-doc/SKILL.md`).
Use `--reload compact` only for sessions that explicitly opt into
`agent_doc_auto_compact` in frontmatter or project `.agent-doc/config.toml`.

## Library target (v0.17.28+)

The crate exposes both a binary and a library (`lib.rs`). The library provides:
- Core modules: `component`, `crdt`, `merge`, `sessions`, `ffi`
- C FFI layer (`src/ffi.rs`) for plugin bindings

Ensure both `cargo test --lib` and `cargo test --bin agent-doc` pass before release.

## Version sync

`pyproject.toml` version must match `Cargo.toml`. Maturin reads `Cargo.toml` as
authoritative but PyPI upload will conflict if `pyproject.toml` drifts.
