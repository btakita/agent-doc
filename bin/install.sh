#!/usr/bin/env bash
# install.sh — Install agent-doc shell scripts and ensure dependencies.
#
# Usage: ./bin/install.sh
#
# Installs:
#   - agent-doc-start → ~/.local/bin/
#   - agent-doc-route → ~/.local/bin/
#   - Ensures tmux is installed (Linux, macOS, Windows/Cygwin)
#   - Ensures jq is installed

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
INSTALL_DIR="${HOME}/.local/bin"

info()  { echo "  [+] $*"; }
warn()  { echo "  [!] $*" >&2; }
error() { echo "  [x] $*" >&2; }

# --- Detect OS ---
detect_os() {
  case "$(uname -s)" in
    Linux*)  echo "linux" ;;
    Darwin*) echo "macos" ;;
    CYGWIN*|MINGW*|MSYS*) echo "windows" ;;
    *)       echo "unknown" ;;
  esac
}

OS=$(detect_os)

# --- Install tmux ---
install_tmux() {
  if command -v tmux &>/dev/null; then
    info "tmux already installed: $(tmux -V)"
    return 0
  fi

  warn "tmux not found — installing..."

  case "$OS" in
    linux)
      if command -v apt-get &>/dev/null; then
        sudo apt-get update -qq && sudo apt-get install -y tmux
      elif command -v dnf &>/dev/null; then
        sudo dnf install -y tmux
      elif command -v pacman &>/dev/null; then
        sudo pacman -S --noconfirm tmux
      elif command -v zypper &>/dev/null; then
        sudo zypper install -y tmux
      elif command -v apk &>/dev/null; then
        sudo apk add tmux
      else
        error "Cannot detect package manager. Install tmux manually."
        return 1
      fi
      ;;
    macos)
      if command -v brew &>/dev/null; then
        brew install tmux
      elif command -v port &>/dev/null; then
        sudo port install tmux
      else
        error "Install Homebrew (https://brew.sh) first, then: brew install tmux"
        return 1
      fi
      ;;
    windows)
      warn "Windows detected. tmux requires a Unix-like environment."
      if command -v choco &>/dev/null; then
        warn "Attempting to install itmux via Chocolatey..."
        choco install itmux -y 2>/dev/null || true
      fi
      if ! command -v tmux &>/dev/null; then
        echo ""
        echo "  Options for tmux on Windows:"
        echo "    1. itmux (standalone): https://github.com/itefixnet/itmux"
        echo "    2. WSL: wsl --install, then: sudo apt install tmux"
        echo "    3. Git Bash + MSYS2: pacman -S tmux"
        echo ""
        return 1
      fi
      ;;
    *)
      error "Unknown OS. Install tmux manually."
      return 1
      ;;
  esac

  if command -v tmux &>/dev/null; then
    info "tmux installed: $(tmux -V)"
  else
    error "tmux installation failed"
    return 1
  fi
}

# --- Install jq ---
install_jq() {
  if command -v jq &>/dev/null; then
    info "jq already installed: $(jq --version)"
    return 0
  fi

  warn "jq not found — installing..."

  case "$OS" in
    linux)
      if command -v apt-get &>/dev/null; then
        sudo apt-get update -qq && sudo apt-get install -y jq
      elif command -v dnf &>/dev/null; then
        sudo dnf install -y jq
      elif command -v pacman &>/dev/null; then
        sudo pacman -S --noconfirm jq
      elif command -v zypper &>/dev/null; then
        sudo zypper install -y jq
      elif command -v apk &>/dev/null; then
        sudo apk add jq
      else
        warn "Cannot detect package manager. Install jq manually (scripts fall back to python3)."
        return 0
      fi
      ;;
    macos)
      if command -v brew &>/dev/null; then
        brew install jq
      else
        warn "Install Homebrew first, then: brew install jq (scripts fall back to python3)."
        return 0
      fi
      ;;
    windows)
      if command -v choco &>/dev/null; then
        choco install jq -y
      else
        warn "Install jq manually or use python3 fallback."
        return 0
      fi
      ;;
  esac

  if command -v jq &>/dev/null; then
    info "jq installed: $(jq --version)"
  fi
}

# --- Symlink scripts to ~/.local/bin ---
install_scripts() {
  mkdir -p "$INSTALL_DIR"

  for script in agent-doc-start agent-doc-route; do
    local src="$SCRIPT_DIR/$script"
    local dst="$INSTALL_DIR/$script"

    if [[ ! -f "$src" ]]; then
      error "Source script not found: $src"
      continue
    fi

    if [[ -L "$dst" ]]; then
      rm "$dst"
    elif [[ -f "$dst" ]]; then
      warn "$dst already exists (not a symlink). Skipping."
      continue
    fi

    ln -s "$src" "$dst"
    info "Linked $script → $dst"
  done

  # Check if ~/.local/bin is in PATH
  if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
    warn "$INSTALL_DIR is not in PATH"
    echo ""
    echo "  Add to your shell profile (~/.bashrc or ~/.zshrc):"
    echo "    export PATH=\"\$PATH:$HOME/.local/bin\""
    echo ""
  fi
}

# --- Main ---
echo "agent-doc installer"
echo "==================="
echo ""

echo "Checking dependencies..."
install_tmux
install_jq
echo ""

echo "Installing scripts..."
install_scripts
echo ""

echo "Done."
