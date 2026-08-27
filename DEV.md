---
project_type: rust-maturin
publication_targets:
  - pypi
  - github-release
secret_paths:
  pypi: "pypi/token"
post_release:
  - "cargo install --path ."
  - "cargo build --release"
---

# agent-doc — Release Notes

## Source checkout prerequisites

The Rust workspace expects sibling checkouts of `tmux-router`, `lazily-rs`, and
`agent-kit`. The JetBrains plugin additionally requires JDK 21, `lazily-kt`, and
the `lazily-spec/proto` sibling when the local `lazily-kt` composite build is
present. `settings.gradle.kts` now fails with that exact missing path instead of
an opaque Gradle variant error.

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

Run `make release-version VERSION=<version>` before updating `VERSIONS.md`.
The `agent-doc-dev` harness projects that one value into all workspace package
versions, internal path-dependency constraints, `Cargo.lock`, `pyproject.toml`,
and both shipped/development `SKILL.md` markers. `make check` runs the matching
projection guard and an isolated harness self-test, so release preparation
cannot defer another skill-marker repair until late in the test suite.
