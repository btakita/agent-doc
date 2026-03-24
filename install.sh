#!/bin/sh
# install.sh — agent-doc installer
#
# Downloads the latest prebuilt binary from GitHub Releases and installs it
# to ~/.cargo/bin/ (if it exists) or ~/.local/bin/ (created if needed).
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/btakita/agent-doc/main/install.sh | sh

set -euo pipefail

REPO="btakita/agent-doc"
GITHUB_API="https://api.github.com/repos/${REPO}/releases/latest"
GITHUB_DL="https://github.com/${REPO}/releases/download"

# ---------------------------------------------------------------------------
# Platform detection
# ---------------------------------------------------------------------------

detect_platform() {
  case "$(uname -s)" in
    Linux)  echo "linux" ;;
    Darwin) echo "macos" ;;
    *)
      echo "ERROR: Unsupported OS: $(uname -s)" >&2
      echo "  See https://github.com/${REPO}/releases for manual download." >&2
      exit 1
      ;;
  esac
}

detect_arch() {
  case "$(uname -m)" in
    x86_64 | amd64)  echo "x86_64" ;;
    aarch64 | arm64) echo "aarch64" ;;
    *)
      echo "ERROR: Unsupported architecture: $(uname -m)" >&2
      echo "  See https://github.com/${REPO}/releases for manual download." >&2
      exit 1
      ;;
  esac
}

# ---------------------------------------------------------------------------
# Install directory selection
# ---------------------------------------------------------------------------

select_install_dir() {
  if [ -d "${HOME}/.cargo/bin" ]; then
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
# Resolve latest release tag from GitHub API
# ---------------------------------------------------------------------------

echo "Fetching latest agent-doc release..."

TAG="$(curl -s "${GITHUB_API}" | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')"

if [ -z "${TAG}" ]; then
  echo "ERROR: Could not determine latest release tag." >&2
  echo "  Check your internet connection or visit: https://github.com/${REPO}/releases" >&2
  exit 1
fi

echo "Latest release: ${TAG}"

# ---------------------------------------------------------------------------
# Build download URL
# Binary name pattern: agent-doc-{tag}-{platform}-{arch}
# e.g. agent-doc-v0.25.12-linux-x86_64
# ---------------------------------------------------------------------------

PLATFORM="$(detect_platform)"
ARCH="$(detect_arch)"

BINARY_NAME="agent-doc-${TAG}-${PLATFORM}-${ARCH}"
DOWNLOAD_URL="${GITHUB_DL}/${TAG}/${BINARY_NAME}"

echo "Platform : ${PLATFORM}"
echo "Arch     : ${ARCH}"
echo "Binary   : ${BINARY_NAME}"
echo "URL      : ${DOWNLOAD_URL}"

# ---------------------------------------------------------------------------
# Select and prepare install directory
# ---------------------------------------------------------------------------

INSTALL_DIR="$(select_install_dir)"

if [ ! -d "${INSTALL_DIR}" ]; then
  echo "Creating install directory: ${INSTALL_DIR}"
  mkdir -p "${INSTALL_DIR}"
fi

INSTALL_PATH="${INSTALL_DIR}/agent-doc"

# ---------------------------------------------------------------------------
# Download binary
# ---------------------------------------------------------------------------

echo ""
echo "Downloading to ${INSTALL_PATH}..."
curl -fSL --progress-bar "${DOWNLOAD_URL}" -o "${INSTALL_PATH}"

# ---------------------------------------------------------------------------
# Make executable
# ---------------------------------------------------------------------------

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
# PATH guidance (only shown when install dir is not already on PATH)
# ---------------------------------------------------------------------------

case ":${PATH}:" in
  *":${INSTALL_DIR}:"*)
    # Already on PATH — nothing to print
    ;;
  *)
    echo ""
    echo "NOTE: ${INSTALL_DIR} is not in your PATH."
    echo "      To use agent-doc from any directory, add it to your PATH:"
    echo ""
    echo "        export PATH=\"${INSTALL_DIR}:\$PATH\""
    echo ""
    echo "      Add that line to your shell profile to make it permanent:"
    echo "        ~/.bashrc   (Bash)"
    echo "        ~/.zshrc    (Zsh)"
    echo ""
    echo "      Then reload your shell or run:"
    echo ""
    echo "        source ~/.bashrc   # or ~/.zshrc"
    ;;
esac
