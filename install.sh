#!/bin/sh
# install.sh — agent-doc installer
#
# Downloads the latest prebuilt binary from GitHub Releases and installs it
# to ~/.cargo/bin/ (if it exists) or ~/.local/bin/ (created if needed).
#
# Supports: Linux (x86_64, aarch64), macOS (x86_64, aarch64), Windows (x86_64)
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/btakita/agent-doc/main/install.sh | sh
#
# Options (via environment variables):
#   AGENT_DOC_INSTALL_DIR  Override install directory (default: ~/.cargo/bin or ~/.local/bin)
#   AGENT_DOC_VERSION      Install a specific version (default: latest)

set -eu

REPO="btakita/agent-doc"
GITHUB_API="https://api.github.com/repos/${REPO}/releases/latest"
GITHUB_DL="https://github.com/${REPO}/releases/download"

# ---------------------------------------------------------------------------
# Platform detection
# ---------------------------------------------------------------------------

detect_target() {
  OS="$(uname -s)"
  ARCH="$(uname -m)"

  case "$OS" in
    Linux)
      case "$ARCH" in
        x86_64 | amd64)
          if is_musl; then
            echo "x86_64-unknown-linux-musl"
          else
            echo "x86_64-unknown-linux-gnu"
          fi
          ;;
        aarch64 | arm64) echo "aarch64-unknown-linux-gnu" ;;
        *) unsupported "$OS" "$ARCH" ;;
      esac
      ;;
    Darwin)
      case "$ARCH" in
        x86_64 | amd64)  echo "x86_64-apple-darwin" ;;
        aarch64 | arm64) echo "aarch64-apple-darwin" ;;
        *) unsupported "$OS" "$ARCH" ;;
      esac
      ;;
    MINGW* | MSYS* | CYGWIN*)
      case "$ARCH" in
        x86_64 | amd64) echo "x86_64-pc-windows-msvc" ;;
        *) unsupported "$OS" "$ARCH" ;;
      esac
      ;;
    *) unsupported "$OS" "$ARCH" ;;
  esac
}

is_musl() {
  # Prefer the active loader's own report. A glibc host can have an optional
  # musl loader installed for compatibility; its mere presence must not switch
  # the selected release asset away from GNU (GH #363).
  if command -v ldd >/dev/null 2>&1; then
    LDD_OUTPUT="$(ldd --version 2>&1 || true)"
    case "$LDD_OUTPUT" in
      *musl*) return 0 ;;
      *glibc* | *GLIBC* | *"GNU libc"*) return 1 ;;
    esac
  fi

  # Some minimal musl systems provide a loader but no useful `ldd --version`.
  for loader in /lib/ld-musl-*.so.1 /usr/lib/ld-musl-*.so.1; do
    if [ -e "$loader" ]; then
      return 0
    fi
  done
  return 1
}

unsupported() {
  echo "ERROR: Unsupported platform: $1 $2" >&2
  echo "  See https://github.com/${REPO}/releases for manual download." >&2
  exit 1
}

is_windows() {
  case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*) return 0 ;;
    *) return 1 ;;
  esac
}

# ---------------------------------------------------------------------------
# Install directory selection
# ---------------------------------------------------------------------------

select_install_dir() {
  if [ -n "${AGENT_DOC_INSTALL_DIR:-}" ]; then
    echo "$AGENT_DOC_INSTALL_DIR"
  elif [ -d "${HOME}/.cargo/bin" ]; then
    echo "${HOME}/.cargo/bin"
  else
    echo "${HOME}/.local/bin"
  fi
}

# ---------------------------------------------------------------------------
# Dependency checks
# ---------------------------------------------------------------------------

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "ERROR: Required command not found: $1" >&2
    exit 1
  fi
}

need_cmd curl
need_cmd grep
need_cmd sed

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | sed 's/[[:space:]].*$//'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | sed 's/[[:space:]].*$//'
  elif command -v openssl >/dev/null 2>&1; then
    openssl dgst -sha256 "$1" | sed 's/^.*= //'
  else
    echo "ERROR: A SHA-256 tool is required (sha256sum, shasum, or openssl)." >&2
    exit 1
  fi
}

checksum_from_sidecar() {
  while read -r digest listed_name _rest; do
    listed_name="${listed_name#\*}"
    case "$digest" in
      '' | *[!0-9A-Fa-f]*) continue ;;
    esac
    if [ "${#digest}" -eq 64 ] && [ "$listed_name" = "$ARCHIVE_NAME" ]; then
      echo "$digest"
      return 0
    fi
  done < "$1"
  return 1
}

# ---------------------------------------------------------------------------
# Resolve release tag
# ---------------------------------------------------------------------------

echo "Installing agent-doc..."
echo ""

if [ -n "${AGENT_DOC_VERSION:-}" ]; then
  TAG="$AGENT_DOC_VERSION"
  echo "Requested version: ${TAG}"
else
  echo "Fetching latest release..."
  TAG="$(curl -s "${GITHUB_API}" | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')"

  if [ -z "${TAG}" ]; then
    echo "ERROR: Could not determine latest release tag." >&2
    echo "  Check your internet connection or visit: https://github.com/${REPO}/releases" >&2
    exit 1
  fi
  echo "Latest release: ${TAG}"
fi

# ---------------------------------------------------------------------------
# Build download URL
# Release assets: agent-doc-{target}.tar.gz (Unix) or agent-doc-{target}.zip (Windows)
# ---------------------------------------------------------------------------

TARGET="$(detect_target)"

if is_windows; then
  ARCHIVE_NAME="agent-doc-${TARGET}.zip"
  BINARY_NAME="agent-doc.exe"
else
  ARCHIVE_NAME="agent-doc-${TARGET}.tar.gz"
  BINARY_NAME="agent-doc"
fi

DOWNLOAD_URL="${GITHUB_DL}/${TAG}/${ARCHIVE_NAME}"
CHECKSUM_URL="${DOWNLOAD_URL}.sha256"

echo "Target   : ${TARGET}"
echo "Archive  : ${ARCHIVE_NAME}"

# ---------------------------------------------------------------------------
# Select and prepare install directory
# ---------------------------------------------------------------------------

INSTALL_DIR="$(select_install_dir)"

if [ ! -d "${INSTALL_DIR}" ]; then
  echo "Creating install directory: ${INSTALL_DIR}"
  mkdir -p "${INSTALL_DIR}"
fi

INSTALL_PATH="${INSTALL_DIR}/${BINARY_NAME}"

# ---------------------------------------------------------------------------
# Download and extract
# ---------------------------------------------------------------------------

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

echo ""
echo "Downloading ${DOWNLOAD_URL}..."
curl -fSL --progress-bar "${DOWNLOAD_URL}" -o "${TMP_DIR}/${ARCHIVE_NAME}"
echo "Downloading ${CHECKSUM_URL}..."
curl -fSL --progress-bar "${CHECKSUM_URL}" -o "${TMP_DIR}/${ARCHIVE_NAME}.sha256"

if ! EXPECTED_SHA256="$(checksum_from_sidecar "${TMP_DIR}/${ARCHIVE_NAME}.sha256")"; then
  echo "ERROR: Checksum sidecar has no valid entry for ${ARCHIVE_NAME}." >&2
  exit 1
fi
ACTUAL_SHA256="$(sha256_file "${TMP_DIR}/${ARCHIVE_NAME}")"
if [ "$ACTUAL_SHA256" != "$EXPECTED_SHA256" ]; then
  echo "ERROR: Integrity check failed for ${ARCHIVE_NAME}; refusing to install." >&2
  echo "  Expected: ${EXPECTED_SHA256}" >&2
  echo "  Actual  : ${ACTUAL_SHA256}" >&2
  exit 1
fi
echo "Checksum verified: ${ARCHIVE_NAME}"

if is_windows; then
  # Windows: unzip
  need_cmd unzip
  unzip -q "${TMP_DIR}/${ARCHIVE_NAME}" -d "${TMP_DIR}"
else
  # Unix: extract tar.gz
  tar xzf "${TMP_DIR}/${ARCHIVE_NAME}" -C "${TMP_DIR}"
fi

# Move binary to install directory
mv "${TMP_DIR}/${BINARY_NAME}" "${INSTALL_PATH}"
chmod +x "${INSTALL_PATH}"

# Install the native library beside the binary. The editor plugins load it
# through `agent-doc lib-path`, which resolves it as a sibling of the
# executable; without it they fall back to the degraded file-based IPC mode
# (GH #52 put it in the archive, but this script used to discard it). Copy to a
# temp name in the install directory, then rename, so an IDE that has the
# previous library mapped keeps its old inode.
for LIB_NAME in libagent_doc.so libagent_doc.dylib agent_doc.dll; do
  if [ -f "${TMP_DIR}/${LIB_NAME}" ]; then
    cp "${TMP_DIR}/${LIB_NAME}" "${INSTALL_DIR}/.${LIB_NAME}.tmp"
    mv -f "${INSTALL_DIR}/.${LIB_NAME}.tmp" "${INSTALL_DIR}/${LIB_NAME}"
    echo "Installed native library: ${INSTALL_DIR}/${LIB_NAME}"
  fi
done

# ---------------------------------------------------------------------------
# Verify installation
# ---------------------------------------------------------------------------

echo ""
echo "Verifying installation..."
if "${INSTALL_PATH}" --version; then
  echo ""
  echo "agent-doc installed successfully to ${INSTALL_PATH}"
else
  echo "ERROR: Installed binary failed to run." >&2
  echo "  The downloaded binary may not be compatible with this system." >&2
  exit 1
fi

# ---------------------------------------------------------------------------
# PATH guidance
# ---------------------------------------------------------------------------

case ":${PATH}:" in
  *":${INSTALL_DIR}:"*)
    ;;
  *)
    echo ""
    echo "NOTE: ${INSTALL_DIR} is not in your PATH."
    echo "      Add it with:"
    echo ""
    echo "        export PATH=\"${INSTALL_DIR}:\$PATH\""
    echo ""
    echo "      Add to ~/.bashrc or ~/.zshrc to make permanent."
    ;;
esac

# ---------------------------------------------------------------------------
# Next steps
# ---------------------------------------------------------------------------

echo ""
echo "Next steps (Getting started: https://github.com/${REPO}#getting-started):"
echo "  cd your-project                         # a git repository"
echo "  agent-doc init                          # set up .agent-doc/ for the project"
echo "  agent-doc skill install --harness claude  # or: codex, grok, opencode, --all"
echo "  agent-doc plugin install jetbrains      # or: vscode (optional)"
echo "  agent-doc init notes.md \"My Topic\"      # create your first session document"
