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
        x86_64 | amd64)  echo "x86_64-unknown-linux-gnu" ;;
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
