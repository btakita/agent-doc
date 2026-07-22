#!/usr/bin/env bash

set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/reclaim-disk-space.sh [OPTIONS]

Find disposable Cargo build directories below a project root. The default is a
dry run: nothing is deleted until --execute is supplied.

Options:
  --root PATH       Scan PATH instead of the current directory.
  --user-caches     Also include selected rebuildable developer caches.
  --execute         Delete the listed directories.
  -h, --help        Show this help.

Examples:
  scripts/reclaim-disk-space.sh --root .
  scripts/reclaim-disk-space.sh --root . --execute
  scripts/reclaim-disk-space.sh --root . --user-caches --execute
EOF
}

execute=false
include_user_caches=false
root=$PWD

while (($#)); do
  case "$1" in
    --root)
      [[ $# -ge 2 ]] || { printf 'error: --root requires a path\n' >&2; exit 2; }
      root=$2
      shift 2
      ;;
    --user-caches)
      include_user_caches=true
      shift
      ;;
    --execute)
      execute=true
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      printf 'error: unknown option: %s\n' "$1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

root=$(cd -- "$root" && pwd -P)
if [[ $root == / || $root == "$HOME" ]]; then
  printf 'error: refusing an overly broad project root: %s\n' "$root" >&2
  exit 2
fi

declare -a paths=()
while IFS= read -r -d '' candidate; do
  parent=${candidate%/target}
  [[ -f "$parent/Cargo.toml" ]] && paths+=("$candidate")
done < <(
  find "$root" \
    -type d \( -name .git -o -name node_modules -o -name .venv -o -name .tsift \) -prune -o \
    -type d -name target -print0
)

if $include_user_caches; then
  paths+=(
    "$HOME/.cache/go-build"
    "$HOME/.cache/gopls"
    "$HOME/.cache/pip"
    "$HOME/.cache/zig"
    "$HOME/.npm/_cacache"
  )
fi

if $execute; then
  printf 'Deleting disposable build artifacts and caches:\n'
else
  printf 'Dry run; rerun with --execute to delete:\n'
fi

cargo_busy=false
if $execute && command -v pgrep >/dev/null 2>&1; then
  if pgrep -x cargo >/dev/null || pgrep -x rustc >/dev/null; then
    cargo_busy=true
    printf '  note: Cargo/Rust compilation is active; Cargo target directories will be skipped\n'
  fi
fi

found=false
failed=false
for path in "${paths[@]}"; do
  [[ -e $path ]] || continue
  found=true
  if ! size=$(du -sh -- "$path" 2>/dev/null | awk '{print $1}'); then
    size=unknown
  fi
  printf '  %8s  %s\n' "${size:-unknown}" "$path"
  if $execute; then
    if $cargo_busy && [[ ${path##*/} == target ]] && [[ -f ${path%/target}/Cargo.toml ]]; then
      printf '            skipped: active Cargo/Rust compilation\n'
    elif ! rm -rf -- "$path"; then
      failed=true
      printf '            warning: could not completely remove this path\n' >&2
    fi
  fi
done

$found || printf '  nothing eligible found\n'

if $execute; then
  printf '\nCurrent filesystem usage:\n'
  df -h -- "$root"
fi

if $failed; then
  exit 1
fi
