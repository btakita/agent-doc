#!/usr/bin/env bash
# Configure a tmux window to match this agent-loop Codex workspace window:
#   - window name: node
#   - two panes stacked vertically with even height
#   - both panes rooted at the agent-loop repo
#   - top pane selected
#
# The current pane's running command is Codex via node. This script does not
# start Codex by default, because rerunning agents should be an explicit choice.

set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: tmux-match-this-window.sh [options] [TARGET]

Configure TARGET, or the current tmux window when TARGET is omitted, to match
the captured agent-loop Codex tmux window layout.

Options:
  --target TARGET       tmux pane/window target to configure
  --cwd DIR            working directory for shell panes
  --name NAME          tmux window name
  --active-pane PANE   pane to select after setup: top or bottom
  --start-codex        queue `codex` in the top pane after setup
  --force              close extra panes when TARGET has more than two panes
  -h, --help           show this help

Examples:
  bin/tmux-match-this-window.sh
  bin/tmux-match-this-window.sh --target 1:3 --force
  bin/tmux-match-this-window.sh --name zsh --active-pane bottom
USAGE
}

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

repo_root() {
  local script_dir
  script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
  cd -- "$script_dir/.." && pwd -P
}

is_shell_command() {
  case "$(basename -- "$1")" in
    sh|bash|zsh|fish|dash|ksh) return 0 ;;
    *) return 1 ;;
  esac
}

shell_cd_command() {
  local dir="$1"
  printf 'cd %q' "$dir"
}

TARGET="${TMUX_MATCH_TARGET:-}"
WORKDIR="${TMUX_MATCH_CWD:-$(repo_root)}"
WINDOW_NAME="${TMUX_MATCH_WINDOW_NAME:-node}"
ACTIVE_PANE_INDEX=0
START_CODEX=false
FORCE=false

while (($#)); do
  case "$1" in
    --target)
      [[ $# -ge 2 ]] || die "--target requires a value"
      TARGET="$2"
      shift 2
      ;;
    --cwd)
      [[ $# -ge 2 ]] || die "--cwd requires a value"
      WORKDIR="$2"
      shift 2
      ;;
    --name)
      [[ $# -ge 2 ]] || die "--name requires a value"
      WINDOW_NAME="$2"
      shift 2
      ;;
    --active-pane)
      [[ $# -ge 2 ]] || die "--active-pane requires top or bottom"
      case "$2" in
        top) ACTIVE_PANE_INDEX=0 ;;
        bottom) ACTIVE_PANE_INDEX=1 ;;
        *) die "--active-pane must be top or bottom" ;;
      esac
      shift 2
      ;;
    --start-codex)
      START_CODEX=true
      shift
      ;;
    --force)
      FORCE=true
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    --)
      shift
      break
      ;;
    -*)
      die "unknown option: $1"
      ;;
    *)
      [[ -z "$TARGET" ]] || die "unexpected argument: $1"
      TARGET="$1"
      shift
      ;;
  esac
done

[[ $# -eq 0 ]] || die "unexpected argument: $1"
command -v tmux >/dev/null 2>&1 || die "tmux is not installed"
[[ -d "$WORKDIR" ]] || die "working directory does not exist: $WORKDIR"

if [[ -z "$TARGET" ]]; then
  [[ -n "${TMUX_PANE:-}" ]] || die "not inside tmux; pass --target session:window"
  TARGET="$TMUX_PANE"
fi

WINDOW_TARGET="$(tmux display-message -p -t "$TARGET" '#{session_name}:#{window_index}')"

pane_count() {
  tmux list-panes -t "$WINDOW_TARGET" -F '#{pane_id}' | wc -l | tr -d '[:space:]'
}

kill_extra_panes() {
  local keep_pane pane
  keep_pane="$(tmux display-message -p -t "$TARGET" '#{pane_id}')"
  while IFS= read -r pane; do
    [[ "$pane" == "$keep_pane" ]] && continue
    tmux kill-pane -t "$pane"
  done < <(tmux list-panes -t "$WINDOW_TARGET" -F '#{pane_id}')
}

count="$(pane_count)"
if ((count > 2)); then
  if [[ "$FORCE" != true ]]; then
    die "$WINDOW_TARGET has $count panes; rerun with --force to close extras"
  fi
  kill_extra_panes
  count="$(pane_count)"
fi

if ((count == 1)); then
  tmux split-window -t "$WINDOW_TARGET" -v -c "$WORKDIR"
elif ((count != 2)); then
  die "expected one or two panes after cleanup, found $count"
fi

tmux rename-window -t "$WINDOW_TARGET" "$WINDOW_NAME"
tmux select-layout -t "$WINDOW_TARGET" even-vertical >/dev/null

mapfile -t PANES < <(tmux list-panes -t "$WINDOW_TARGET" -F '#{pane_index} #{pane_id} #{pane_current_command}')

TOP_PANE=""
ACTIVE_PANE=""
for pane_info in "${PANES[@]}"; do
  read -r pane_index pane_id pane_command <<<"$pane_info"
  [[ "$pane_index" == "0" ]] && TOP_PANE="$pane_id"
  [[ "$pane_index" == "$ACTIVE_PANE_INDEX" ]] && ACTIVE_PANE="$pane_id"

  if is_shell_command "$pane_command"; then
    tmux send-keys -t "$pane_id" "$(shell_cd_command "$WORKDIR")" C-m
  fi
done

[[ -n "$TOP_PANE" ]] || die "could not find top pane in $WINDOW_TARGET"
[[ -n "$ACTIVE_PANE" ]] || die "could not find requested active pane in $WINDOW_TARGET"

if [[ "$START_CODEX" == true ]]; then
  top_command="$(tmux display-message -p -t "$TOP_PANE" '#{pane_current_command}')"
  if ! is_shell_command "$top_command"; then
    die "top pane is running $top_command; refusing to queue codex"
  fi
  tmux send-keys -t "$TOP_PANE" "$(shell_cd_command "$WORKDIR") && codex" C-m
fi

tmux select-pane -t "$ACTIVE_PANE"
tmux select-layout -t "$WINDOW_TARGET" even-vertical >/dev/null

printf 'Configured tmux window %s as %s: two even vertical panes in %s\n' \
  "$WINDOW_TARGET" "$WINDOW_NAME" "$WORKDIR"
