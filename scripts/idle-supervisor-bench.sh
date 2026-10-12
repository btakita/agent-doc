#!/usr/bin/env bash
# Idle supervisor/controller CPU bench (`#fixthrashingbackground`).
#
# Starts N real `agent-doc start` supervisors (each over a fake `claude`
# harness) in an ISOLATED tmux server against a scratch project, lets them go
# idle, then measures for WINDOW seconds:
#   - CPU ticks (utime+stime, /proc) of every supervisor and of the scratch
#     project's controller,
#   - ops.log lines written during the window, by event name.
# Nothing here touches a live project, a live tmux server, or a live process:
# the project, tmux socket, HOME, and controller are all scratch, and teardown
# only kills the processes this script started.
#
# Usage: scripts/idle-supervisor-bench.sh <agent-doc-binary> <work-dir> [N] [WINDOW] [SETTLE]
set -euo pipefail

BIN=$(readlink -f "$1")
WORK=$2
N=${3:-4}
# Extra `agent-doc start` flags, e.g. the route-owned shape live panes run:
#   START_ARGS="--route-owned --route-owned-reap-policy keep-alive --route-owned-start-purpose layout-provision"
START_ARGS=${START_ARGS:-}
WINDOW=${4:-60}
SETTLE=${5:-20}

rm -rf "$WORK"
mkdir -p "$WORK"
WORK=$(readlink -f "$WORK")
ROOT="$WORK/project"
BIN_DIR="$ROOT/.fake-bin"
HOME_DIR="$WORK/home"
SOCKET="idlebench-$$"
mkdir -p "$ROOT/.agent-doc" "$BIN_DIR" "$HOME_DIR"

cat >"$BIN_DIR/claude" <<'PY'
#!/usr/bin/env python3
# Fake Claude Code harness: renders an idle frame, records input, never works.
import os, select, sys, termios
try:
    attrs = termios.tcgetattr(0)
    attrs[3] &= ~termios.ECHO
    termios.tcsetattr(0, termios.TCSANOW, attrs)
except termios.error:
    pass
d = os.environ["FAKE_CLAUDE_DIR"]
os.makedirs(d, exist_ok=True)
open(os.path.join(d, "started"), "w").close()
RULE = "─" * 40 + "\n"
def idle():
    sys.stdout.write("\x1b[2J\x1b[H" + RULE + "❯ \n" + RULE
                     + "  Opus 4.8 ctx:40% ~/fake main\n  ⏵⏵ bypass permissions on (shift+tab to cycle)\n"
                     + "\x1b[2;3H")
    sys.stdout.flush()
idle()
ticks = 0
while True:
    ready, _, _ = select.select([sys.stdin], [], [], 0.1)
    ticks += 1
    if not ready and ticks % 10 == 0:
        idle()
    if ready and not sys.stdin.readline():
        break
PY
chmod 755 "$BIN_DIR/claude"
ln -s "$BIN" "$BIN_DIR/agent-doc"

for i in $(seq 1 "$N"); do
  sid=$(printf '00000000-0000-4000-8000-%012d' "$i")
  {
    printf -- '---\nagent_doc_session: %s\nagent: claude\n---\n\n# Bench %s\n\n' "$sid" "$i"
    printf '<!-- agent:queue -->\n<!-- /agent:queue -->\n\n## User\n\nhello\n\n'
    # A realistically sized document: the per-tick cost of anything that
    # re-reads or re-parses it scales with its size.
    for j in $(seq 1 400); do
      printf -- '- note %s: lorem ipsum dolor sit amet, `code span %s` consectetur adipiscing elit\n' "$j" "$j"
    done
  } >"$ROOT/doc$i.md"
done
git -C "$ROOT" init -q
git -C "$ROOT" config user.email bench@example.com
git -C "$ROOT" config user.name Bench
git -C "$ROOT" add .
git -C "$ROOT" commit -qm init

PATH_ENV="$BIN_DIR:$PATH"
for i in $(seq 1 "$N"); do
  cat >"$BIN_DIR/run-$i.sh" <<SH
#!/bin/sh
export PATH='$PATH_ENV'
export HOME='$HOME_DIR'
export FAKE_CLAUDE_DIR='$WORK/fake-$i'
export AGENT_DOC_TMUX_SOCKET='$SOCKET'
export AGENT_DOC_ROOT_CEILING_DIRECTORIES='$WORK'
unset CLAUDECODE
if [ "\$(command -v claude)" != '$BIN_DIR/claude' ]; then echo refusing; sleep 600; exit 99; fi
cd '$ROOT'
exec agent-doc start ${START_ARGS:-} doc$i.md
SH
  chmod 755 "$BIN_DIR/run-$i.sh"
done

cleanup() {
  [ -n "${KEEP:-}" ] && return 0
  tmux -L "$SOCKET" kill-server 2>/dev/null || true
  rm -f "/tmp/tmux-$(id -u)/$SOCKET"
  for pid in $(pgrep -f "controller serve --project-root $ROOT" || true); do
    kill "$pid" 2>/dev/null || true
  done
}
trap cleanup EXIT

env -u TMUX -u TMUX_PANE PATH="$PATH_ENV" HOME="$HOME_DIR" tmux -f /dev/null -L "$SOCKET" new-session -d -s main -x 200 -y 50 "$BIN_DIR/run-1.sh"
for i in $(seq 2 "$N"); do
  env -u TMUX -u TMUX_PANE PATH="$PATH_ENV" HOME="$HOME_DIR" tmux -f /dev/null -L "$SOCKET" new-window -t main "$BIN_DIR/run-$i.sh"
done


# Safety: never let a REAL harness run against the scratch project.
real_harness_guard() {
  local pid cwd
  for pid in $(pgrep -x claude 2>/dev/null); do
    case "$(readlink "/proc/$pid/exe" 2>/dev/null || true)" in
      *python*) continue ;;
    esac
    cwd=$(readlink "/proc/$pid/cwd" 2>/dev/null || true)
    case "$cwd" in
      "$ROOT"*) echo "bench: a real claude ($pid) started in the scratch project; aborting" >&2; exit 2 ;;
    esac
  done
}
deadline=$((SECONDS + 90))
until [ "$(ls "$WORK"/fake-*/started 2>/dev/null | wc -l)" -ge "$N" ]; do
  real_harness_guard
  if [ "$SECONDS" -ge "$deadline" ]; then
    echo "bench: harnesses did not start" >&2
    exit 1
  fi
  sleep 1
done
sleep "$SETTLE"

ticks() {
  local total=0 pid
  for pid in "$@"; do
    [ -r "/proc/$pid/stat" ] || continue
    total=$((total + $(awk '{print $14 + $15}' "/proc/$pid/stat")))
  done
  echo "$total"
}
SUPS=$(pgrep -f "agent-doc start .*doc[0-9]+\.md" | tr '\n' ' ')
CTRL=$(pgrep -f "controller serve --project-root $ROOT" | tr '\n' ' ')
OPS="$ROOT/.agent-doc/logs/ops.log"
lines_before=$(wc -l <"$OPS" 2>/dev/null || echo 0)
sup_before=$(ticks $SUPS)
ctrl_before=$(ticks $CTRL)
sleep "$WINDOW"
sup_after=$(ticks $SUPS)
ctrl_after=$(ticks $CTRL)
hz=$(getconf CLK_TCK)

echo "binary=$BIN supervisors=$N window_s=$WINDOW settle_s=$SETTLE"
echo "supervisor_pids=$SUPS controller_pids=$CTRL"
awk -v s=$((sup_after - sup_before)) -v c=$((ctrl_after - ctrl_before)) -v hz="$hz" -v n="$N" -v w="$WINDOW" 'BEGIN {
  printf "supervisor_cpu_ms_total=%d per_supervisor_cpu_ms_per_idle_min=%.0f\n", s*1000/hz, s*1000/hz/n*60/w
  printf "controller_cpu_ms_total=%d controller_cpu_ms_per_idle_min=%.0f\n", c*1000/hz, c*1000/hz*60/w
}'
echo "ops_log_lines_in_window_by_event:"
tail -n +"$((lines_before + 1))" "$OPS" 2>/dev/null \
  | sed -E 's/^\[[^]]*\] ([a-z_]+).*/\1/' | sort | uniq -c | sort -rn | head -20
echo "controller_crdt_current_text_in_window=$(tail -n +"$((lines_before + 1))" "$OPS" 2>/dev/null | grep -c ' controller_crdt_current_text ' || true)"
