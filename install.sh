#!/bin/sh
# agent-doc installer — downloads prebuilt binary from GitHub Releases
#
# Usage:
#   curl -sSf https://raw.githubusercontent.com/btakita/agent-doc/main/install.sh | sh
#   curl -sSf ... | sh -s -- --system          # install to /usr/local/bin
#   curl -sSf ... | sh -s -- --version 0.1.0   # specific version
set -eu

REPO="btakita/agent-doc"
INSTALL_DIR="$HOME/.local/bin"
USE_SUDO=""
VERSION=""

usage() {
    cat <<EOF
Usage: install.sh [OPTIONS]

Options:
  --system          Install to /usr/local/bin (requires sudo)
  --version VER     Install a specific version (e.g. 0.1.0)
  --help            Show this help
EOF
    exit 0
}

while [ $# -gt 0 ]; do
    case "$1" in
        --system)
            INSTALL_DIR="/usr/local/bin"
            USE_SUDO="sudo"
            shift
            ;;
        --version)
            VERSION="$2"
            shift 2
            ;;
        --help)
            usage
            ;;
        *)
            echo "Unknown option: $1" >&2
            usage
            ;;
    esac
done

# Detect OS
OS="$(uname -s)"
case "$OS" in
    Linux)  OS_TARGET="unknown-linux-gnu" ;;
    Darwin) OS_TARGET="apple-darwin" ;;
    *)
        echo "Unsupported OS: $OS" >&2
        echo "See https://github.com/$REPO/releases for manual download." >&2
        exit 1
        ;;
esac

# Detect architecture
ARCH="$(uname -m)"
case "$ARCH" in
    x86_64|amd64)   ARCH_TARGET="x86_64" ;;
    aarch64|arm64)   ARCH_TARGET="aarch64" ;;
    *)
        echo "Unsupported architecture: $ARCH" >&2
        echo "See https://github.com/$REPO/releases for manual download." >&2
        exit 1
        ;;
esac

TARGET="${ARCH_TARGET}-${OS_TARGET}"

# Resolve version
if [ -z "$VERSION" ]; then
    echo "Fetching latest release..."
    VERSION="$(curl -sSf "https://api.github.com/repos/$REPO/releases/latest" \
        | grep '"tag_name"' \
        | sed 's/.*"tag_name": *"v\{0,1\}\([^"]*\)".*/\1/')"
    if [ -z "$VERSION" ]; then
        echo "Failed to determine latest version." >&2
        exit 1
    fi
fi

TAG="v$VERSION"
ARCHIVE="agent-doc-${TARGET}.tar.gz"
URL="https://github.com/$REPO/releases/download/$TAG/$ARCHIVE"

echo "Installing agent-doc $VERSION for $TARGET..."
echo "  From: $URL"
echo "  To:   $INSTALL_DIR/agent-doc"

# Create temp directory
TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

# Download and extract
curl -sSfL "$URL" -o "$TMPDIR/$ARCHIVE"
tar xzf "$TMPDIR/$ARCHIVE" -C "$TMPDIR"

# Install binary
mkdir -p "$INSTALL_DIR"
$USE_SUDO install -m 755 "$TMPDIR/agent-doc" "$INSTALL_DIR/agent-doc"

# Verify
if "$INSTALL_DIR/agent-doc" --version >/dev/null 2>&1; then
    echo "agent-doc $VERSION installed successfully."
    "$INSTALL_DIR/agent-doc" --version
else
    echo "Warning: agent-doc installed but failed to run." >&2
    echo "Check that $INSTALL_DIR/agent-doc is executable." >&2
fi

# PATH hint
case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        echo ""
        echo "Note: $INSTALL_DIR is not in your PATH."
        echo "Add it with:"
        echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
        echo ""
        echo "To make this permanent, add the line above to your ~/.bashrc or ~/.zshrc."
        ;;
esac
