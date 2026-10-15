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

## Build priority and parallelism

Every `make` invocation runs agent-doc's builds and tests below interactive
work (`#keepidesnappy`), so concurrent worktree checks do not make the editor
lag. At parse time the Makefile lowers its own process to `BUILD_NICE` and the
`BUILD_IONICE_CLASS` I/O class. Every recipe, compiler, test binary, and
Gradle or TLC JVM inherits that priority. It only ever lowers priority. It then
sizes parallelism from the cores that are actually idle
(`scripts/build-priority.py`).

| Variable | Default | Effect |
|---|---|---|
| `AGENT_DOC_BUILD_PRIORITY` | `low` | `normal` leaves nice, I/O class, and Gradle priority untouched |
| `BUILD_NICE` | `19` | nice level for make and everything it starts |
| `BUILD_IONICE_CLASS` | `3` (idle) | I/O scheduling class |
| `BUILD_JOBS` | `cores - cores/8 - load1`, clamped to `[max(2, cores/8), cores - cores/8]` | base parallelism |
| `CARGO_BUILD_JOBS` | `BUILD_JOBS` | Cargo `-j` |
| `NEXTEST_TEST_THREADS` | `BUILD_JOBS` | nextest test threads |
| `BATCHED_TEST_JOBS` | `BUILD_JOBS / 2` | concurrent batched test binaries (each runs `TEST_THREADS`) |
| `GRADLE_MAX_WORKERS` | `BUILD_JOBS` | Gradle `--max-workers`; Gradle also gets `--priority=low` |
| `TLA_JOBS` / `TLA_WORKERS` | `BUILD_JOBS / 4` (max 8) / `BUILD_JOBS` | concurrent TLC JVMs / workers per JVM |

An explicit value in the environment or on the `make` command line always
wins. `make build-priority-probe` prints what a recipe actually runs under, and
`make build-priority-self-test` (part of `check-fast` and `check`) proves the
wiring. Machine and IDE settings that only the operator can change are
covered in [IDE responsiveness](ide-responsiveness.md).

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
