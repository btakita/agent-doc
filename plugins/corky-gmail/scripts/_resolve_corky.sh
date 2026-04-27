#!/usr/bin/env bash
set -euo pipefail

resolve_corky_bin() {
  if [[ -n "${CORKY_BIN:-}" ]]; then
    printf '%s\n' "$CORKY_BIN"
    return 0
  fi

  if command -v corky >/dev/null 2>&1; then
    command -v corky
    return 0
  fi

  local script_dir repo_root
  script_dir="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
  repo_root="$(CDPATH= cd -- "$script_dir/../../.." && pwd)"

  if [[ -x "$repo_root/src/corky/.bin/corky" ]]; then
    printf '%s\n' "$repo_root/src/corky/.bin/corky"
    return 0
  fi

  if [[ -x "$repo_root/src/corky/target/release/corky" ]]; then
    printf '%s\n' "$repo_root/src/corky/target/release/corky"
    return 0
  fi

  echo "corky binary not found; install corky, build src/corky/.bin/corky, or set CORKY_BIN" >&2
  return 127
}
