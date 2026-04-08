---
description: Scaffold a new Rust package with CI, release workflows, Makefile, and release profile
user-invocable: true
argument-hint: "<name> [--path <path>]"
---

# rust-package

Scaffold a production-ready Rust binary package following btakita conventions. Creates or updates the standard project infrastructure.

## Invocation

```
/rust-package <name>                  — scaffold in src/<name>/
/rust-package <name> --path <path>    — scaffold at custom path
```

## What It Creates

### 1. Cargo.toml — Release Profile

Add if missing:

```toml
[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
panic = "abort"
strip = true
```

### 2. Makefile — Standard Targets

```makefile
.PHONY: build release test clippy check install install-hooks clean

build:
	cargo build

release:
	cargo build --release
	@mkdir -p .bin
	@ln -sf ../target/release/<name> .bin/<name>
	@echo "Installed .bin/<name> -> target/release/<name>"

test:
	cargo test

clippy:
	cargo clippy -- -D warnings

check: clippy test

install:
	cargo install --path .

install-hooks:
	@mkdir -p .git/hooks
	@printf '#!/bin/sh\nmake check\n' > .git/hooks/pre-commit
	@chmod +x .git/hooks/pre-commit
	@echo "Installed .git/hooks/pre-commit"

publish:
	cargo publish

clean:
	cargo clean
	rm -f .bin/<name>
```

If the project has `audit-docs` or other pre-commit tasks, add a `precommit` target:
```makefile
precommit: check
	cargo run --quiet -- audit-docs
```

### 3. GitHub Actions — CI Workflow

`.github/workflows/ci.yml`:

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      # Path deps resolved from crates.io (local dev uses .cargo/config.toml patch)

      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy

      - run: cargo clippy -- -D warnings

      - run: cargo test
```

### 4. GitHub Actions — Release Workflow

`.github/workflows/release.yml`:

Triggered on tag push (`v*`). Builds cross-platform binaries and creates a GitHub Release.

**Build matrix:**

| Target | OS | Cross |
|--------|----|----|
| x86_64-unknown-linux-gnu | ubuntu-latest | no |
| aarch64-unknown-linux-gnu | ubuntu-latest | yes (cross v0.2.5) |
| x86_64-apple-darwin | macos-14 | no |
| aarch64-apple-darwin | macos-latest | no |
| x86_64-pc-windows-msvc | windows-latest | no |

**Packaging:** `tar.gz` for Unix, `.zip` for Windows.

**Release job:** Downloads all artifacts, creates GitHub Release with `gh release create`.

### 5. Optional: PyPI Workflow

If the project uses `maturin` (has `pyproject.toml`), add `.github/workflows/pypi.yml`:
- Build wheels for Linux (x86_64), macOS (x86_64 + aarch64), Windows (x86_64)
- Publish via OIDC to PyPI

### 6. Optional: install.sh

Standalone installer for end-users:
- Detect platform (Linux/macOS) and architecture (x86_64/aarch64)
- Download binary from GitHub Releases
- Install to `~/.cargo/bin/` or `~/.local/bin/`

## Release Checklist

1. Bump version in `Cargo.toml` (+ `pyproject.toml` if applicable)
2. `make check` (clippy + test)
3. Commit and push to main
4. Tag: `git tag v<version> && git push origin v<version>`
5. CI triggers release workflow → builds binaries → creates GitHub Release
6. `cargo publish` (crates.io)
7. `cargo install --path .` (update local binary)

## Conventions

- **Secrets:** Never inline cleartext secrets. Use `pass` subshell or env vars.
- **Path dependencies:** Use `.cargo/config.toml` `[patch.crates-io]` for local development (gitignored). `Cargo.toml` references crate versions only — CI resolves from crates.io, no cloning needed.
- **Edition:** Use `edition = "2024"` for new projects.
- **License:** Default to `MIT OR Apache-2.0` for open source.
- **Binary symlink:** `make release` creates `.bin/<name>` symlink for agent-loop workspace.
