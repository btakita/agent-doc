# Installation

## pip / pipx (all platforms)

```sh
pip install agent-doc
# or
pipx install agent-doc
```

This installs a prebuilt wheel with the compiled binary — no Rust toolchain needed.

## Shell installer (Linux & macOS)

```sh
curl -sSf https://raw.githubusercontent.com/btakita/agent-doc/main/install.sh | sh
```

This downloads a prebuilt binary to `~/.local/bin/agent-doc`. Use `--system` to install to `/usr/local/bin` instead (requires sudo).

## From source

```sh
cargo install --path .
```

## Windows

`pip install agent-doc` is the easiest option. Alternatively, download `.zip` from [GitHub Releases](https://github.com/btakita/agent-doc/releases) or build from source with `cargo install --path .`.
