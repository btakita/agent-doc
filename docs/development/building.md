# Building

## Developer setup

```sh
git clone https://github.com/btakita/agent-doc.git
cd agent-doc
make release    # build + symlink to .bin/agent-doc
```

## Make targets

```sh
make build        # Debug build
make release      # Release build + symlink to .bin/agent-doc
make test         # Run tests
make clippy       # Lint
make check        # Lint + test
make precommit    # Full pre-commit checks (lint + test + audit-docs)
make install      # Install to ~/.cargo/bin
make init-python  # Set up Python venv with maturin
make wheel        # Build wheel and install into venv
```

## .gitignore

The following are gitignored:

```
target/
.bin/
.agent-doc/
.venv/
CLAUDE.local.md
.idea/
```

## Release build

The release profile optimizes for binary size and performance:

```toml
[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
panic = "abort"
strip = true
```
