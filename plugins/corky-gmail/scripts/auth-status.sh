#!/usr/bin/env bash
set -euo pipefail

script_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
# shellcheck source=./_resolve_corky.sh
source "$script_dir/_resolve_corky.sh"

exec "$(resolve_corky_bin)" doctor gmail --json "$@"
