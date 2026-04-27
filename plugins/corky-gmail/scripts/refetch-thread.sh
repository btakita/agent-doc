#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: refetch-thread.sh THREAD_ID [extra corky sync refetch args...]" >&2
  exit 64
fi

thread_id="$1"
shift

script_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
# shellcheck source=./_resolve_corky.sh
source "$script_dir/_resolve_corky.sh"

exec "$(resolve_corky_bin)" sync refetch "$thread_id" --json "$@"
