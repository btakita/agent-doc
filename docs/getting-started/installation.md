# Installation

New here? Use the shell installer below, then follow the
[Quick Start](quick-start.md). Use PyPI on Windows or where Python tooling is
already how you install CLIs. Build from source only to work on agent-doc
itself. Every method installs the same `agent-doc` binary plus the native
library (`libagent_doc.so` / `.dylib` / `agent_doc.dll`) that the editor plugins
load; `agent-doc version` shows both.

## Shell installer (Linux, macOS, Windows via a POSIX shell)

```sh
curl -fsSL https://raw.githubusercontent.com/btakita/agent-doc/main/install.sh | sh
```

This downloads the latest prebuilt binary and its native library from
[GitHub Releases](https://github.com/btakita/agent-doc/releases) into
`~/.cargo/bin` (if it exists) or `~/.local/bin`. Set `AGENT_DOC_INSTALL_DIR` to
choose another directory, or `AGENT_DOC_VERSION` to pin a release. If the
directory is not on your `PATH`, the installer prints the line to add.

Release archives appear independently: each platform archive is public as soon
as that platform build succeeds, alongside an archive-specific `.sha256`
sidecar. If one platform fails, successful platforms remain downloadable while
the Actions run remains red for a precise “Re-run failed jobs” retry. Retries
replace the same asset names instead of creating duplicates. The aggregate
`SHA256SUMS` file appears only
after all six platform archives verify, and continues to mark readiness for
PyPI and Homebrew.

`agent-doc upgrade` verifies its selected archive against that archive-specific
sidecar, so it can use a completed platform pair before the aggregate manifest
appears. If a newer release is visible but its archive or sidecar is not yet
available for this platform, the one-shot command exits non-zero and leaves the
installed binary and plugins unchanged.

The shell installer uses the same archive-specific sidecar before extracting or
replacing anything. On Linux x86_64 it chooses the GNU or musl archive from the
active C library; merely having an optional musl compatibility loader installed
does not redirect a glibc host to the musl asset.

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

`agent-doc upgrade` on published Linux and macOS targets follows a checksummed
per-target stable pointer. A ready Coder/Linux target can therefore advance
before unrelated Darwin or Windows builds finish. The pointer also binds the
release's complete JetBrains package manifest, so Remote Dev backend and Client
compatibility are promoted together. Invalid or mid-update pointer evidence
fails closed and never falls through to an unpinned PyPI version. Windows keeps
the aggregate global-stable/manual path because a running `.exe` needs a future
detached replacement helper for safe self-upgrade.
Existing installations begin using this channel only after they have installed
a release that contains the target-aware resolver; older clients deliberately
continue to follow GitHub's aggregate global `latest` release.

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
none is installed. JetBrains builds use the Lazily version pinned in their Gradle
build files even when a `lazily-kt` checkout happens to be adjacent. Set
`AGENT_DOC_LAZILY_KT_DIR=/path/to/lazily-kt` to opt into a local composite build;
that checkout requires an adjacent `lazily-spec/proto` sibling.

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
