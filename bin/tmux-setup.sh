#!/bin/sh

# Usage: tmux-setup.sh [--dev] [--claude]
# No flags = both windows. One flag = only that window.

run_dev=false
run_claude=false

for arg in "$@"; do
  case "$arg" in
    --dev) run_dev=true ;;
    --claude) run_claude=true ;;
    *) echo "Usage: tmux-setup.sh [--dev] [--claude]"; exit 1 ;;
  esac
done

# Default: both windows if no flags given
if [ "$run_dev" = false ] && [ "$run_claude" = false ]; then
  run_dev=true
  run_claude=true
fi

if [ "$run_dev" = true ]; then
  tmux rename-window "dev"
  tmux send-keys -t "cd ~/work/btakita/agent-loop" Enter
  tmux split-window -v -f
  tmux split-window -v -f
  tmux split-window -v -f
  tmux select-layout even-vertical
  tmux select-pane -t 0

  tmux send-keys -t :dev.1 "cd agent-doc" Enter
  tmux send-keys -t :dev.2 "cd corky" Enter
  tmux send-keys -t :dev.3 "cd resume" Enter
fi

if [ "$run_claude" = true ]; then
  tmux new-window -n "claude"
  tmux send-keys -t "cd ~/work/btakita/agent-loop" Enter
  tmux split-window -h -f
  tmux select-layout even-horizontal
  tmux select-pane -t 0

  tmux send-keys -t :claude.0 "agent-doc start tasks/plan.md" Enter
  tmux send-keys -t :claude.1 "agent-doc start tasks/resume.md" Enter
fi
