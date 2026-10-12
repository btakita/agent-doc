# Installation

## Shell installer (Linux, macOS, Windows via a POSIX shell)

```sh
curl -fsSL https://raw.githubusercontent.com/btakita/agent-doc/main/install.sh | sh
```

This downloads the latest prebuilt binary from
[GitHub Releases](https://github.com/btakita/agent-doc/releases) into
`~/.cargo/bin` (if it exists) or `~/.local/bin`. Set `AGENT_DOC_INSTALL_DIR` to
choose another directory, or `AGENT_DOC_VERSION` to pin a release.

## PyPI

```sh
pip install agent-doc
# or
pipx install agent-doc
```

The PyPI package is a small, platform-independent bootstrap. On its first run it
downloads the executable and FFI library for that exact package version from the
matching GitHub Release, verifies the release's `SHA256SUMS`, and caches the two
files under the user cache directory. Later runs work from that versioned cache.
On Linux, an explicit libc result from Python selects the matching GNU or musl
asset; an installed musl compatibility loader cannot override a detected glibc
host.

## Homebrew

```sh
brew install btakita/tap/agent-doc
```

Linux today; macOS once the release's Darwin archives are uploaded.

## From source

```sh
# From a superproject checkout that vendors agent-doc under src/agent-doc
cargo install --path src/agent-doc --force

# Or from inside the agent-doc checkout
cargo install --path . --force
```

`make install` also rebuilds already-installed editor packages. The JetBrains
build runs its Gradle daemon on JDK 21; Gradle provisions a Temurin 21 when
none is installed. In a monorepo checkout, an initialized sibling `lazily-kt`
development build also requires sibling `lazily-spec/proto`; standalone checkouts
use the published Lazily dependency instead.

The Rust workspace is an implementation detail: every agent-doc Cargo package is
marked `publish = false`. New releases ship through GitHub Releases and PyPI;
older crates.io uploads remain as immutable historical artifacts.

## Windows

`pip install agent-doc` is the easiest option. Alternatively, download the
Windows `.zip` from [GitHub Releases](https://github.com/btakita/agent-doc/releases)
or build from source with `cargo install --path .`.

## Upgrading

`agent-doc upgrade --auto` captures its original executable and installer type
before the first replacement. Later polls keep upgrading that exact path even
when the watcher's process image is shown as `(deleted)`, and standalone release
installs never fall back to `pip`.

## Editor plugins and agent harnesses

- Editor plugins: `agent-doc plugin install jetbrains` or
  `agent-doc plugin install vscode`; Zed uses a dev extension. See
  [Editor Integration](../guide/editor-integration.md).
- Agent harness instructions: `agent-doc skill install --harness <claude|codex|grok|opencode|cursor|generic>`,
  or `--all`. See [Agent Backends](../guide/agent-backends.md).
