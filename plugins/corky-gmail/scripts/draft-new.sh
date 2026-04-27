#!/usr/bin/env bash
set -euo pipefail

in_reply_to=""
thread_id=""
args=("$@")

while [[ $# -gt 0 ]]; do
  case "$1" in
    --in-reply-to)
      shift
      [[ $# -gt 0 ]] || { echo "--in-reply-to requires a value" >&2; exit 64; }
      in_reply_to="$1"
      ;;
    --thread-id)
      shift
      [[ $# -gt 0 ]] || { echo "--thread-id requires a value" >&2; exit 64; }
      thread_id="$1"
      ;;
  esac
  shift
done

if [[ -n "$thread_id" && -z "$in_reply_to" ]]; then
  echo "threaded Gmail replies require both --thread-id and --in-reply-to" >&2
  exit 64
fi

if [[ -n "$thread_id" && "$in_reply_to" == "$thread_id" ]]; then
  echo "do not pass the Gmail thread ID as --in-reply-to; use the RFC 2822 Message-ID there" >&2
  exit 64
fi

if [[ -n "$in_reply_to" && "$in_reply_to" != *"@"* ]]; then
  echo "warning: --in-reply-to does not look like an RFC 2822 Message-ID" >&2
fi

script_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
# shellcheck source=./_resolve_corky.sh
source "$script_dir/_resolve_corky.sh"

exec "$(resolve_corky_bin)" draft new "${args[@]}"
