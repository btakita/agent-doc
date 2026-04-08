# Cargo Path Dependencies — Local Override Pattern

Use `.cargo/config.toml` to override crate dependencies with local paths during development, while keeping `Cargo.toml` clean for CI and publishing.

## Pattern

**`Cargo.toml`** — crate versions only (no `path =`):
```toml
agent-kit = "0.3"
instruction-files = "0.1"
```

**`.cargo/config.toml`** — local path overrides (gitignored):
```toml
[patch.crates-io]
agent-kit = { path = "../agent-kit" }
instruction-files = { path = "../instruction-files" }
```

**`.gitignore`:**
```
.cargo/
```

## Why

- `cargo publish` rejects path deps — this pattern avoids that
- CI resolves from crates.io — no `git clone` steps needed
- Local development uses the sibling checkout — changes propagate immediately
- `Cargo.lock` tracks the resolved version (crate or local)

## When to apply

- Any Rust package in `src/` that depends on another local crate (agent-kit, instruction-files, tmux-router, etc.)
- When setting up a new package with `/rust-package`

## Existing usage

- `src/agent-doc/` — patches `tmux-router`
- `src/corky/` — patches `agent-kit`, `instruction-files`
