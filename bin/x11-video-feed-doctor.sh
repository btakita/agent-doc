#!/usr/bin/env bash
#
# Diagnose an X11 video-call feed and isolate CPU-heavy processes from a set
# of reserved logical CPUs. Changes made by `fix` are recorded so `restore`
# can undo CPU-affinity changes and resume processes suspended by this script.

set -uo pipefail

PROGRAM="${0##*/}"
MODE="check"
CAMERA=""
MIN_WIDTH=1280
MIN_HEIGHT=720
MIN_FPS=24
CPU_THRESHOLD=150
MIN_PROCESS_AGE=10
MAX_OFFENDERS=3
RESERVE_CPUS=2
SUSPEND_OFFENDERS=false
RESTART_CAMERA=false
DRY_RUN=false
CHECK_NETWORK=true
PING_HOST="8.8.8.8"
MEET_WINDOW=""
MEET_WINDOW_REGEX='Meet.*(Chromium|Chrome|Brave|Firefox)'
STATE_FILE="${XDG_RUNTIME_DIR:-/tmp}/x11-video-feed-doctor-${UID}.state"

ISSUES=0
WARNINGS=0
ACTIONS=0
X11_CORE=""
CAMERA_WIDTH=""
CAMERA_HEIGHT=""
CAMERA_FPS=""
CAMERA_PIXEL_FORMAT=""

declare -a PROTECTED_PIDS=()
declare -a CAMERA_OWNER_PIDS=()
declare -a OFFENDER_PIDS=()
declare -a OFFENDER_CORES=()
declare -a OFFENDER_CPUS=()
declare -a OFFENDER_COMMANDS=()

usage() {
  cat <<'USAGE'
Usage:
  x11-video-feed-doctor.sh [check] [options]
  x11-video-feed-doctor.sh fix [options]
  x11-video-feed-doctor.sh restore [--state-file FILE]

Modes:
  check      diagnose X11, camera, network, and competing processes (default)
  fix        reserve CPUs for interactive/video work and move CPU hogs away
  restore    restore affinities and resume processes changed by `fix`

Fixes are deliberately scoped to process-caused feed problems. The script
does not click through a meeting application's settings or change accounts.

Options:
  --camera DEVICE          V4L2 camera device (default: first /dev/videoN)
  --min-width N            minimum healthy capture width (default: 1280)
  --min-height N           minimum healthy capture height (default: 720)
  --min-fps N              minimum healthy frame rate (default: 24)
  --cpu-threshold PERCENT  flag a process at this CPU percentage (default: 150)
  --min-process-age SEC    ignore newer transient processes (default: 10)
  --max-offenders N        maximum processes changed by fix (default: 3)
  --reserve-cpus N         logical CPUs left free of offenders (default: 2)
  --suspend                also SIGSTOP offenders; `restore` sends SIGCONT
  --restart-camera         toggle Meet camera off/on after process isolation
  --meet-window XID        explicit Meet X11 window for --restart-camera
  --meet-window-regex RE   X11 title regex used to locate Meet
  --state-file FILE        repair state path
  --ping-host HOST         network probe host (default: 8.8.8.8)
  --no-network             skip the ICMP network check
  --dry-run                print repair actions without applying them
  -h, --help               show this help

Examples:
  bin/x11-video-feed-doctor.sh
  bin/x11-video-feed-doctor.sh fix --cpu-threshold 120
  bin/x11-video-feed-doctor.sh fix --suspend --restart-camera
  bin/x11-video-feed-doctor.sh restore

Exit status:
  0  healthy check, successful fix, or successful restore
  1  one or more issues detected, or no applicable fix was available
  2  invalid arguments or a required repair dependency is unavailable
USAGE
}

say() {
  printf '%s\n' "$*"
}

ok() {
  printf '[ok] %s\n' "$*"
}

warn() {
  WARNINGS=$((WARNINGS + 1))
  printf '[warn] %s\n' "$*" >&2
}

issue() {
  ISSUES=$((ISSUES + 1))
  printf '[issue] %s\n' "$*" >&2
}

die() {
  printf '[error] %s\n' "$*" >&2
  exit 2
}

have() {
  command -v "$1" >/dev/null 2>&1
}

require_value() {
  local option="$1"
  local count="$2"
  ((count >= 2)) || die "$option requires a value"
}

require_uint() {
  local option="$1"
  local value="$2"
  [[ "$value" =~ ^[0-9]+$ ]] || die "$option requires a non-negative integer"
}

float_at_least() {
  awk -v actual="$1" -v minimum="$2" 'BEGIN { exit !(actual + 0 >= minimum + 0) }'
}

float_less_than() {
  awk -v actual="$1" -v minimum="$2" 'BEGIN { exit !(actual + 0 < minimum + 0) }'
}

trim() {
  local value="$1"
  value="${value#"${value%%[![:space:]]*}"}"
  value="${value%"${value##*[![:space:]]}"}"
  printf '%s' "$value"
}

add_protected_pid() {
  local candidate="${1:-}"
  [[ "$candidate" =~ ^[0-9]+$ ]] || return 0
  ((candidate > 0)) || return 0
  PROTECTED_PIDS+=("$candidate")
}

pid_is_protected() {
  local pid="$1"
  local protected
  for protected in "${PROTECTED_PIDS[@]}"; do
    [[ "$pid" == "$protected" ]] && return 0
  done
  return 1
}

protect_ancestors() {
  local pid="$$"
  local parent
  while [[ "$pid" =~ ^[0-9]+$ ]] && ((pid > 1)); do
    add_protected_pid "$pid"
    parent="$(ps -o ppid= -p "$pid" 2>/dev/null || true)"
    parent="$(trim "$parent")"
    [[ "$parent" =~ ^[0-9]+$ ]] || break
    [[ "$parent" != "$pid" ]] || break
    pid="$parent"
  done
}

detect_camera() {
  local device
  [[ -n "$CAMERA" ]] && return 0
  while IFS= read -r device; do
    [[ -e "$device" ]] || continue
    CAMERA="$device"
    return 0
  done < <(compgen -G '/dev/video[0-9]*' | sort -V)
  return 1
}

proc_start_ticks() {
  local pid="$1"
  local stat_line
  local stat_tail
  local -a fields
  IFS= read -r stat_line < "/proc/$pid/stat" 2>/dev/null || return 1
  stat_tail="${stat_line##*) }"
  read -r -a fields <<< "$stat_tail"
  ((${#fields[@]} > 19)) || return 1
  printf '%s' "${fields[19]}"
}

process_matches_start() {
  local pid="$1"
  local expected="$2"
  local actual
  [[ -d "/proc/$pid" ]] || return 1
  actual="$(proc_start_ticks "$pid" 2>/dev/null || true)"
  [[ -n "$actual" && "$actual" == "$expected" ]]
}

read_affinity() {
  local pid="$1"
  local output
  output="$(taskset -pc "$pid" 2>/dev/null)" || return 1
  output="${output##*:}"
  trim "$output"
}

expand_cpu_list() {
  local cpu_list="$1"
  local segment
  local first
  local last
  local cpu
  local IFS=','
  read -r -a segments <<< "$cpu_list"
  for segment in "${segments[@]}"; do
    if [[ "$segment" == *-* ]]; then
      first="${segment%-*}"
      last="${segment#*-}"
      [[ "$first" =~ ^[0-9]+$ && "$last" =~ ^[0-9]+$ ]] || return 1
      for ((cpu = first; cpu <= last; cpu++)); do
        printf '%s\n' "$cpu"
      done
    else
      [[ "$segment" =~ ^[0-9]+$ ]] || return 1
      printf '%s\n' "$segment"
    fi
  done
}

background_cpu_list() {
  local allowed
  local reserve="$RESERVE_CPUS"
  local reserved_count=0
  local index
  local cpu
  local result=""
  local -a cpus
  local -A reserved=()

  allowed="$(read_affinity "$$")" || return 1
  mapfile -t cpus < <(expand_cpu_list "$allowed")
  ((${#cpus[@]} >= 2)) || return 1

  if ((reserve >= ${#cpus[@]})); then
    reserve=$((${#cpus[@]} - 1))
  fi
  ((reserve >= 1)) || reserve=1

  if [[ "$X11_CORE" =~ ^[0-9]+$ ]]; then
    for cpu in "${cpus[@]}"; do
      if [[ "$cpu" == "$X11_CORE" ]]; then
        reserved["$cpu"]=1
        reserved_count=1
        break
      fi
    done
  fi

  for ((index = ${#cpus[@]} - 1; index >= 0 && reserved_count < reserve; index--)); do
    cpu="${cpus[$index]}"
    [[ -n "${reserved[$cpu]:-}" ]] && continue
    reserved["$cpu"]=1
    reserved_count=$((reserved_count + 1))
  done

  for cpu in "${cpus[@]}"; do
    [[ -n "${reserved[$cpu]:-}" ]] && continue
    [[ -z "$result" ]] || result+=","
    result+="$cpu"
  done
  printf '%s' "$result"
}

collect_x11_state() {
  local pid
  local row
  local x11_pid=""
  local x11_cpu=""

  if [[ -z "${DISPLAY:-}" ]]; then
    issue 'DISPLAY is not set; no X11 session is visible'
    return
  fi

  if have xdpyinfo && ! xdpyinfo -display "$DISPLAY" >/dev/null 2>&1; then
    issue "cannot query X11 display $DISPLAY"
    return
  fi
  ok "X11 display $DISPLAY is reachable"

  while IFS= read -r pid; do
    [[ "$pid" =~ ^[0-9]+$ ]] || continue
    add_protected_pid "$pid"
    [[ -n "$x11_pid" ]] || x11_pid="$pid"
  done < <(pgrep -x Xorg 2>/dev/null; pgrep -x Xwayland 2>/dev/null)

  if [[ -n "$x11_pid" ]]; then
    row="$(ps -o psr=,pcpu= -p "$x11_pid" 2>/dev/null || true)"
    read -r X11_CORE x11_cpu <<< "$row"
    ok "X11 server pid=$x11_pid current-core=${X11_CORE:-unknown} cpu=${x11_cpu:-unknown}%"
  else
    warn 'Xorg/Xwayland server process was not visible'
  fi

  if have xdotool; then
    if [[ -n "$MEET_WINDOW" ]]; then
      if xdotool getwindowname "$MEET_WINDOW" >/dev/null 2>&1; then
        ok "Meet window $MEET_WINDOW is reachable"
      else
        warn "Meet window $MEET_WINDOW is not reachable"
      fi
    else
      MEET_WINDOW="$(xdotool search --onlyvisible --name "$MEET_WINDOW_REGEX" 2>/dev/null | head -n 1 || true)"
      if [[ -n "$MEET_WINDOW" ]]; then
        ok "found Meet X11 window $MEET_WINDOW"
      else
        warn "no visible Meet window matched: $MEET_WINDOW_REGEX"
      fi
    fi
  else
    warn 'xdotool is unavailable; Meet window discovery is disabled'
  fi
}

collect_camera_state() {
  local details
  local resolution
  local owner

  if ! detect_camera; then
    issue 'no /dev/videoN camera device was found'
    return
  fi
  [[ -e "$CAMERA" ]] || {
    issue "camera device does not exist: $CAMERA"
    return
  }
  have v4l2-ctl || {
    issue 'v4l2-ctl is required to inspect the negotiated camera format'
    return
  }

  details="$(v4l2-ctl -d "$CAMERA" --get-fmt-video --get-parm 2>&1)" || {
    issue "cannot query camera $CAMERA: ${details//$'\n'/ }"
    return
  }

  resolution="$(
    awk -F ':' '/Width\/Height/ {
      value=$2
      gsub(/[[:space:]]/, "", value)
      print value
      exit
    }' <<< "$details"
  )"
  CAMERA_WIDTH="${resolution%/*}"
  CAMERA_HEIGHT="${resolution#*/}"
  CAMERA_FPS="$(
    awk -F ':' '/Frames per second/ {
      value=$2
      sub(/^[[:space:]]+/, "", value)
      sub(/[[:space:]].*$/, "", value)
      print value
      exit
    }' <<< "$details"
  )"
  CAMERA_PIXEL_FORMAT="$(
    awk -F ':' '/Pixel Format/ {
      value=$2
      gsub(/[[:space:]'\'']/, "", value)
      print value
      exit
    }' <<< "$details"
  )"

  if [[ "$CAMERA_WIDTH" =~ ^[0-9]+$ && "$CAMERA_HEIGHT" =~ ^[0-9]+$ ]]; then
    if ((CAMERA_WIDTH < MIN_WIDTH || CAMERA_HEIGHT < MIN_HEIGHT)); then
      issue "$CAMERA is ${CAMERA_WIDTH}x${CAMERA_HEIGHT}; expected at least ${MIN_WIDTH}x${MIN_HEIGHT}"
    else
      ok "$CAMERA is ${CAMERA_WIDTH}x${CAMERA_HEIGHT} ${CAMERA_PIXEL_FORMAT:-unknown-format}"
    fi
  else
    issue "could not parse the negotiated resolution for $CAMERA"
  fi

  if [[ "$CAMERA_FPS" =~ ^[0-9]+([.][0-9]+)?$ ]]; then
    if float_less_than "$CAMERA_FPS" "$MIN_FPS"; then
      issue "$CAMERA is ${CAMERA_FPS} fps; expected at least ${MIN_FPS} fps"
    else
      ok "$CAMERA is ${CAMERA_FPS} fps"
    fi
  else
    warn "could not parse the negotiated frame rate for $CAMERA"
  fi

  if have fuser; then
    while IFS= read -r owner; do
      [[ "$owner" =~ ^[0-9]+$ ]] || continue
      CAMERA_OWNER_PIDS+=("$owner")
      add_protected_pid "$owner"
    done < <(fuser "$CAMERA" 2>/dev/null | tr ' ' '\n')
    if ((${#CAMERA_OWNER_PIDS[@]})); then
      ok "camera owner pid(s): ${CAMERA_OWNER_PIDS[*]}"
    else
      warn "$CAMERA is not currently opened by a visible process"
    fi
  fi
}

collect_network_state() {
  local ping_output
  local loss
  local latency

  "$CHECK_NETWORK" || return 0
  have ping || {
    warn 'ping is unavailable; network health was not checked'
    return
  }

  ping_output="$(ping -c 4 -W 2 "$PING_HOST" 2>&1)" || true
  loss="$(
    awk -F ',' '/packet loss/ {
      value=$3
      gsub(/[^0-9.]/, "", value)
      print value
      exit
    }' <<< "$ping_output"
  )"
  latency="$(
    awk '/^rtt |^round-trip / {
      sub(/^.*=[[:space:]]*/, "")
      split($0, values, "/")
      print values[2]
      exit
    }' <<< "$ping_output"
  )"

  if [[ "$loss" =~ ^[0-9]+([.][0-9]+)?$ ]]; then
    if float_at_least "$loss" 1; then
      issue "network probe to $PING_HOST has ${loss}% packet loss"
    else
      ok "network probe: ${loss}% loss, ${latency:-unknown} ms average latency"
    fi
  else
    warn "network probe to $PING_HOST did not return packet-loss data"
  fi
}

process_name_is_protected() {
  local comm="$1"
  local args="$2"
  case "$comm" in
    Xorg|Xwayland|chromium|chrome|google-chrome|brave|brave-browser|firefox|\
    pipewire|wireplumber|pulseaudio|kwin_x11|kwin_wayland|gnome-shell|\
    plasmashell|picom|obs|obs-studio|ffmpeg|zoom|teams|slack|\
    xdg-desktop-portal*|systemd|init|dbus-daemon)
      return 0
      ;;
  esac
  [[ "$args" == *"$PROGRAM"* ]]
}

collect_offenders() {
  local row
  local pid
  local core
  local cpu
  local nice
  local state
  local age
  local comm
  local args
  local same_core

  while IFS= read -r row; do
    read -r pid core cpu nice state age comm args <<< "$row"
    [[ "$pid" =~ ^[0-9]+$ ]] || continue
    [[ "$cpu" =~ ^[0-9]+([.][0-9]+)?$ ]] || continue
    [[ "$age" =~ ^[0-9]+$ ]] || continue
    float_at_least "$cpu" "$CPU_THRESHOLD" || continue
    ((age >= MIN_PROCESS_AGE)) || continue
    pid_is_protected "$pid" && continue
    process_name_is_protected "$comm" "${args:-}" && continue
    [[ "$state" != *T* ]] || continue

    OFFENDER_PIDS+=("$pid")
    OFFENDER_CORES+=("$core")
    OFFENDER_CPUS+=("$cpu")
    OFFENDER_COMMANDS+=("$comm")
  done < <(
    ps -u "$UID" -o pid=,psr=,pcpu=,ni=,stat=,etimes=,comm=,args= --sort=-pcpu 2>/dev/null
  )

  if ((${#OFFENDER_PIDS[@]} == 0)); then
    ok "no unprotected process exceeds ${CPU_THRESHOLD}% CPU"
    return
  fi

  issue "${#OFFENDER_PIDS[@]} unprotected process(es) exceed ${CPU_THRESHOLD}% CPU"
  for ((pid = 0; pid < ${#OFFENDER_PIDS[@]} && pid < MAX_OFFENDERS; pid++)); do
    same_core="no"
    [[ -n "$X11_CORE" && "${OFFENDER_CORES[$pid]}" == "$X11_CORE" ]] && same_core="yes"
    printf '        pid=%s cpu=%s%% core=%s same-as-x11=%s command=%s\n' \
      "${OFFENDER_PIDS[$pid]}" \
      "${OFFENDER_CPUS[$pid]}" \
      "${OFFENDER_CORES[$pid]}" \
      "$same_core" \
      "${OFFENDER_COMMANDS[$pid]}"
  done
}

write_state_record() {
  local action="$1"
  local pid="$2"
  local start="$3"
  local value="$4"
  local comm="$5"

  if "$DRY_RUN"; then
    return 0
  fi
  umask 077
  [[ ! -L "$STATE_FILE" ]] || die "refusing to write symlink state file: $STATE_FILE"
  mkdir -p -- "$(dirname -- "$STATE_FILE")" || return 1
  printf '%s\t%s\t%s\t%s\t%s\n' "$action" "$pid" "$start" "$value" "$comm" >> "$STATE_FILE"
}

fix_offenders() {
  local background
  local limit
  local index
  local pid
  local comm
  local start
  local old_affinity

  ((${#OFFENDER_PIDS[@]})) || {
    warn 'no CPU offender is available to isolate'
    return
  }
  have taskset || die 'taskset is required for fix mode'
  background="$(background_cpu_list)" || die 'could not calculate a background CPU set'
  [[ -n "$background" ]] || die 'calculated background CPU set is empty'
  say "[fix] reserving ${RESERVE_CPUS} logical CPU(s); offender CPU set: $background"

  limit="${#OFFENDER_PIDS[@]}"
  ((limit > MAX_OFFENDERS)) && limit="$MAX_OFFENDERS"
  for ((index = 0; index < limit; index++)); do
    pid="${OFFENDER_PIDS[$index]}"
    comm="${OFFENDER_COMMANDS[$index]}"
    [[ -d "/proc/$pid" ]] || {
      warn "pid $pid exited before repair"
      continue
    }
    start="$(proc_start_ticks "$pid" 2>/dev/null || true)"
    old_affinity="$(read_affinity "$pid" 2>/dev/null || true)"
    [[ -n "$start" && -n "$old_affinity" ]] || {
      warn "could not record state for pid $pid ($comm); leaving it unchanged"
      continue
    }

    if [[ "$old_affinity" != "$background" ]]; then
      say "[fix] pid=$pid command=$comm affinity $old_affinity -> $background"
      if "$DRY_RUN"; then
        ACTIONS=$((ACTIONS + 1))
      elif taskset -apc "$background" "$pid" >/dev/null 2>&1; then
        write_state_record "affinity" "$pid" "$start" "$old_affinity" "$comm" || {
          taskset -apc "$old_affinity" "$pid" >/dev/null 2>&1 || true
          die "could not persist repair state to $STATE_FILE"
        }
        ACTIONS=$((ACTIONS + 1))
      else
        warn "could not change affinity for pid $pid ($comm)"
      fi
    else
      warn "pid $pid ($comm) is already confined to the background CPU set"
    fi

    if "$SUSPEND_OFFENDERS"; then
      say "[fix] suspending pid=$pid command=$comm"
      if "$DRY_RUN"; then
        ACTIONS=$((ACTIONS + 1))
      elif kill -STOP "$pid" 2>/dev/null; then
        if write_state_record "suspend" "$pid" "$start" "-" "$comm"; then
          ACTIONS=$((ACTIONS + 1))
        else
          kill -CONT "$pid" 2>/dev/null || true
          die "could not persist repair state to $STATE_FILE"
        fi
      else
        warn "could not suspend pid $pid ($comm)"
      fi
    fi
  done
}

restart_meet_camera() {
  local active_window=""

  "$RESTART_CAMERA" || return 0
  have xdotool || {
    warn 'xdotool is required by --restart-camera'
    return
  }
  [[ -n "$MEET_WINDOW" ]] || {
    warn 'no Meet window is available for --restart-camera'
    return
  }

  say "[fix] toggling Meet camera off/on in window $MEET_WINDOW"
  if "$DRY_RUN"; then
    ACTIONS=$((ACTIONS + 1))
    return
  fi

  active_window="$(xdotool getactivewindow 2>/dev/null || true)"
  if ! xdotool windowactivate --sync "$MEET_WINDOW" 2>/dev/null; then
    warn "could not activate Meet window $MEET_WINDOW"
    return
  fi
  xdotool key --clearmodifiers ctrl+e
  sleep 2
  xdotool key --clearmodifiers ctrl+e
  sleep 3
  if [[ -n "$active_window" && "$active_window" != "$MEET_WINDOW" ]]; then
    xdotool windowactivate "$active_window" 2>/dev/null || true
  fi
  ACTIONS=$((ACTIONS + 1))
}

restore_state() {
  local failures=0
  local index
  local action
  local pid
  local start
  local value
  local comm
  local -a records

  [[ ! -L "$STATE_FILE" ]] || die "refusing to read symlink state file: $STATE_FILE"
  [[ -f "$STATE_FILE" ]] || {
    say "[restore] no state file: $STATE_FILE"
    return 0
  }
  have taskset || die 'taskset is required for restore mode'
  mapfile -t records < "$STATE_FILE"

  for ((index = ${#records[@]} - 1; index >= 0; index--)); do
    IFS=$'\t' read -r action pid start value comm <<< "${records[$index]}"
    [[ "$pid" =~ ^[0-9]+$ && -n "$start" ]] || {
      warn "ignoring malformed state record: ${records[$index]}"
      failures=$((failures + 1))
      continue
    }
    if ! process_matches_start "$pid" "$start"; then
      warn "pid $pid ($comm) no longer identifies the original process"
      continue
    fi

    case "$action" in
      affinity)
        say "[restore] pid=$pid command=$comm affinity -> $value"
        if ! taskset -apc "$value" "$pid" >/dev/null 2>&1; then
          warn "could not restore affinity for pid $pid ($comm)"
          failures=$((failures + 1))
        fi
        ;;
      suspend)
        say "[restore] resuming pid=$pid command=$comm"
        if ! kill -CONT "$pid" 2>/dev/null; then
          warn "could not resume pid $pid ($comm)"
          failures=$((failures + 1))
        fi
        ;;
      *)
        warn "unknown state action: $action"
        failures=$((failures + 1))
        ;;
    esac
  done

  if ((failures == 0)); then
    rm -f -- "$STATE_FILE"
    ok 'repair state restored'
    return 0
  fi
  issue "restore had $failures failure(s); state retained at $STATE_FILE"
  return 1
}

parse_args() {
  if (($#)) && [[ "$1" != -* ]]; then
    case "$1" in
      check|fix|restore)
        MODE="$1"
        shift
        ;;
      *)
        die "unknown mode: $1"
        ;;
    esac
  fi

  while (($#)); do
    case "$1" in
      --camera)
        require_value "$1" "$#"
        CAMERA="$2"
        shift 2
        ;;
      --min-width)
        require_value "$1" "$#"
        require_uint "$1" "$2"
        MIN_WIDTH="$2"
        shift 2
        ;;
      --min-height)
        require_value "$1" "$#"
        require_uint "$1" "$2"
        MIN_HEIGHT="$2"
        shift 2
        ;;
      --min-fps)
        require_value "$1" "$#"
        require_uint "$1" "$2"
        MIN_FPS="$2"
        shift 2
        ;;
      --cpu-threshold)
        require_value "$1" "$#"
        [[ "$2" =~ ^[0-9]+([.][0-9]+)?$ ]] || die "$1 requires a number"
        CPU_THRESHOLD="$2"
        shift 2
        ;;
      --min-process-age)
        require_value "$1" "$#"
        require_uint "$1" "$2"
        MIN_PROCESS_AGE="$2"
        shift 2
        ;;
      --max-offenders)
        require_value "$1" "$#"
        require_uint "$1" "$2"
        MAX_OFFENDERS="$2"
        ((MAX_OFFENDERS >= 1)) || die "$1 must be at least 1"
        shift 2
        ;;
      --reserve-cpus)
        require_value "$1" "$#"
        require_uint "$1" "$2"
        RESERVE_CPUS="$2"
        ((RESERVE_CPUS >= 1)) || die "$1 must be at least 1"
        shift 2
        ;;
      --suspend)
        SUSPEND_OFFENDERS=true
        shift
        ;;
      --restart-camera)
        RESTART_CAMERA=true
        shift
        ;;
      --meet-window)
        require_value "$1" "$#"
        MEET_WINDOW="$2"
        shift 2
        ;;
      --meet-window-regex)
        require_value "$1" "$#"
        MEET_WINDOW_REGEX="$2"
        shift 2
        ;;
      --state-file)
        require_value "$1" "$#"
        STATE_FILE="$2"
        shift 2
        ;;
      --ping-host)
        require_value "$1" "$#"
        PING_HOST="$2"
        shift 2
        ;;
      --no-network)
        CHECK_NETWORK=false
        shift
        ;;
      --dry-run)
        DRY_RUN=true
        shift
        ;;
      -h|--help)
        usage
        exit 0
        ;;
      *)
        die "unknown option: $1"
        ;;
    esac
  done
}

main() {
  parse_args "$@"

  if [[ "$MODE" == "restore" ]]; then
    restore_state
    return $?
  fi

  protect_ancestors
  say "Video feed doctor: $MODE"
  collect_x11_state
  collect_camera_state
  collect_network_state
  collect_offenders

  if [[ "$MODE" == "check" ]]; then
    say "Summary: $ISSUES issue(s), $WARNINGS warning(s)"
    ((ISSUES == 0))
    return
  fi

  fix_offenders
  restart_meet_camera
  if ((ACTIONS == 0)); then
    issue 'no repair action was applied'
    say "Summary: $ISSUES issue(s), $WARNINGS warning(s), 0 repair action(s)"
    return 1
  fi

  if "$DRY_RUN"; then
    say "Summary: dry run would apply $ACTIONS repair action(s)"
  else
    say "Summary: applied $ACTIONS repair action(s)"
    say "Restore with: $PROGRAM restore --state-file $(printf '%q' "$STATE_FILE")"
  fi
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  main "$@"
fi
