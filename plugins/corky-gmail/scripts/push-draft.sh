#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: push-draft.sh FILE [extra corky draft push args...]" >&2
  exit 64
fi

file="$1"
shift

script_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
# shellcheck source=./_resolve_corky.sh
source "$script_dir/_resolve_corky.sh"

exec "$(resolve_corky_bin)" draft push "$file" --json "$@"
