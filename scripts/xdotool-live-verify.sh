#!/usr/bin/env bash
# xdotool-live-verify.sh — drive the #lvbatch live-verification batch by injecting
# real keystrokes into a live IntelliJ/Codex editor with xdotool, then assert the
# per-item IPC/CRDT markers from ops.log. Lets the agent drive AND verify the
# operator-gated live items (#exch-intermix-verify, #postcommit-ipc-worktree-corruption,
# #saevon, #lvbatch markers) without a human typing.
#
# Plan: tasks/agent-doc/plan-xdotool-live-verification.md  (#xdotool-lvbatch)
#
# SAFETY (every run; see plan "Safety guards"):
#   1. Types ONLY into a throwaway scratch doc .agent-doc/live-repro/xdotool-<case>.md,
#      never the real working doc.
#   2. Focus-guards getactivewindow/getwindowname before every type/key; aborts on
#      any mismatch so a stray keystroke cannot land in the wrong window.
#   3. Warns if launched from a degraded/restart-heavy session (verify from a FRESH
#      session — this is where #postcommit-ipc-worktree-corruption / #saevon false
#      successes hide).
#   4. Times keystrokes by an ops.log IPC-apply marker, not a fixed sleep.
#
# Usage:
#   scripts/xdotool-live-verify.sh check-env
#   scripts/xdotool-live-verify.sh list
#   scripts/xdotool-live-verify.sh <case> [--repo <dir>] [--dry-run] [--timeout <sec>]
#
# Cases: exch-intermix | postcommit-worktree | saevon | tmux-switch | lvbatch-markers
#
# This script intentionally contains no agent-doc document logic — all deterministic
# document/commit behavior stays in the binary (CLAUDE.md "All deterministic behavior
# in the binary"). The script only drives live input and greps the binary's own
# ops.log markers.
set -euo pipefail

REPO="${REPO:-$(pwd)}"
DRY_RUN=0
TIMEOUT=30
CASE=""

log()  { printf '[xdotool-live] %s\n' "$*" >&2; }
die()  { printf '[xdotool-live] ERROR: %s\n' "$*" >&2; exit 1; }
warn() { printf '[xdotool-live] WARN: %s\n' "$*" >&2; }

# --- argument parsing -------------------------------------------------------
parse_args() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --repo)    REPO="$2"; shift 2 ;;
      --dry-run) DRY_RUN=1; shift ;;
      --timeout) TIMEOUT="$2"; shift 2 ;;
      -h|--help) sed -n '2,40p' "$0"; exit 0 ;;
      -*)        die "unknown flag: $1" ;;
      *)         if [[ -z "$CASE" ]]; then CASE="$1"; shift; else die "unexpected arg: $1"; fi ;;
    esac
  done
}

ops_log()    { printf '%s/.agent-doc/logs/ops.log' "$REPO"; }
scratch_doc(){ printf '%s/.agent-doc/live-repro/xdotool-%s.md' "$REPO" "$1"; }

# --- environment preflight --------------------------------------------------
check_env() {
  command -v xdotool >/dev/null 2>&1 || die "xdotool not found (install xdotool; ydotool is needed on native Wayland)"
  [[ -n "${DISPLAY:-}" ]] || die "DISPLAY is empty — X11 not reachable; cannot drive live keystrokes"
  if [[ -n "${WAYLAND_DISPLAY:-}" ]]; then
    warn "WAYLAND_DISPLAY is set ($WAYLAND_DISPLAY) — xdotool only drives XWayland windows; native Wayland needs ydotool"
  fi
  if ! xdotool getdisplaygeometry >/dev/null 2>&1; then
    die "xdotool cannot talk to DISPLAY=$DISPLAY (is the X server reachable from this session?)"
  fi
  log "env OK: xdotool=$(xdotool version 2>/dev/null | head -1), DISPLAY=$DISPLAY"
}

# --- fresh-session guard ----------------------------------------------------
# A degraded/restart-heavy session reproduces #postcommit-ipc-worktree-corruption
# on every commit and masks #saevon false-success acks; verifying inside it is
# invalid. Heuristic: count restart markers in the last slice of ops.log.
assert_fresh_session() {
  local olog; olog="$(ops_log)"
  [[ -f "$olog" ]] || { log "no ops.log yet (fresh)"; return 0; }
  local restarts
  restarts=$(tail -n 200 "$olog" 2>/dev/null | grep -c 'restart_supervisor\|session_restart\|supervisor_self_race' || true)
  if (( restarts > 2 )); then
    warn "ops.log shows $restarts recent restart/self-race markers — this looks like a DEGRADED session."
    warn "Per the plan, run live verification from a FRESH session started at the committed boundary."
    [[ "$DRY_RUN" == 1 ]] || die "refusing to drive live keystrokes from a degraded session (use --dry-run to override the gate)"
  fi
}

# --- window resolution + focus guard ----------------------------------------
# Resolve the editor window for the scratch doc by title (basename). Confirms the
# title before returning the id; callers re-confirm focus immediately before typing.
resolve_window() {
  local base="$1" wid title
  for wid in $(xdotool search --name "$base" 2>/dev/null || true); do
    title="$(xdotool getwindowname "$wid" 2>/dev/null || true)"
    if [[ "$title" == *"$base"* ]]; then
      printf '%s' "$wid"
      return 0
    fi
  done
  return 1
}

# Focus-guard: the active window must be the intended scratch editor before any
# type/key. Aborts otherwise so a stray keystroke cannot corrupt real work.
focus_guard() {
  local wid="$1" base="$2" active active_title
  active="$(xdotool getactivewindow 2>/dev/null || true)"
  [[ -n "$active" ]] || die "focus-guard: no active window"
  if [[ "$active" != "$wid" ]]; then
    xdotool windowactivate --sync "$wid" 2>/dev/null || die "focus-guard: cannot activate scratch window $wid"
    active="$(xdotool getactivewindow 2>/dev/null || true)"
  fi
  active_title="$(xdotool getwindowname "$active" 2>/dev/null || true)"
  [[ "$active_title" == *"$base"* ]] \
    || die "focus-guard: active window '$active_title' is not the scratch doc '$base' — ABORT (would corrupt real work)"
}

# --- marker-timed typing ----------------------------------------------------
# Wait for an IPC-apply sentinel to appear in ops.log (timing by marker, not sleep),
# then return so the caller can inject the concurrent edit at the drift window.
wait_for_marker() {
  local marker="$1" olog deadline now
  olog="$(ops_log)"
  deadline=$(( $(date +%s) + TIMEOUT ))
  while :; do
    if [[ -f "$olog" ]] && grep -q -- "$marker" "$olog" 2>/dev/null; then
      return 0
    fi
    now=$(date +%s)
    (( now >= deadline )) && return 1
    sleep 0.1
  done
}

type_into_scratch() {
  local wid="$1" base="$2" text="$3"
  focus_guard "$wid" "$base"
  if [[ "$DRY_RUN" == 1 ]]; then
    log "[dry-run] would type into $wid ($base): '$text'"
    return 0
  fi
  xdotool type --window "$wid" --delay 40 -- "$text"
}

# Assert a per-item marker landed in ops.log within the timeout; report pass/fail.
assert_marker() {
  local item="$1" marker="$2"
  if wait_for_marker "$marker"; then
    log "PASS [$item]: ops.log has '$marker'"
    return 0
  fi
  warn "FAIL [$item]: '$marker' not found in ops.log within ${TIMEOUT}s"
  return 1
}

assert_no_marker() {
  local item="$1" marker="$2" olog
  olog="$(ops_log)"
  if [[ -f "$olog" ]] && grep -q -- "$marker" "$olog" 2>/dev/null; then
    warn "FAIL [$item]: unexpected '$marker' present in ops.log"
    return 1
  fi
  log "PASS [$item]: no '$marker' (as expected)"
  return 0
}

# --- scratch lifecycle ------------------------------------------------------
ensure_scratch_doc() {
  local case_name="$1" doc base
  doc="$(scratch_doc "$case_name")"
  base="$(basename "$doc")"
  mkdir -p "$(dirname "$doc")"
  if [[ ! -f "$doc" ]]; then
    cat >"$doc" <<EOF
---
agent_doc_session: xdotool-${case_name}
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Scratch — #xdotool-lvbatch live-verify ($case_name)
<!-- agent:boundary:initial -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->
EOF
    log "created scratch doc $doc"
  fi
  printf '%s' "$doc"
}

require_window() {
  local base="$1" wid
  if ! wid="$(resolve_window "$base")"; then
    die "no live editor window titled '*$base*' — open the scratch doc in IntelliJ first (this harness drives an already-open editor; it does not launch the IDE)"
  fi
  log "resolved scratch editor window $wid for '$base'"
  printf '%s' "$wid"
}

# --- per-case recipes -------------------------------------------------------
# Each recipe opens/uses the scratch editor, drives a marker-timed concurrent edit,
# then asserts the per-item ops.log marker(s). The deterministic, offline-assertable
# halves (tree==HEAD after closeout; pane move-before-select ordering) live in the
# SimWorld corpus (src/sim_world.rs); these recipes cover the genuinely-live timing
# the simulator cannot exercise.
case_exch_intermix() {
  local doc base wid
  doc="$(ensure_scratch_doc exch-intermix)"; base="$(basename "$doc")"
  wid="$(require_window "$base")"
  log "#exch-intermix-verify: type a mid-finalize edit, expect live_prompt_drift_auto_recovered"
  wait_for_marker "ipc.*apply\|reposition boundary signal sent" || warn "no IPC-apply marker seen before timeout; injecting edit anyway"
  type_into_scratch "$wid" "$base" "mid-finalize concurrent edit"
  assert_marker exch-intermix-verify "live_prompt_drift_auto_recovered" \
    && assert_no_marker exch-intermix-verify "looks like a manual cleanup"
}

case_postcommit_worktree() {
  local doc base wid head_blob tree_blob
  doc="$(ensure_scratch_doc postcommit-worktree)"; base="$(basename "$doc")"
  wid="$(require_window "$base")"
  log "#postcommit-ipc-worktree-corruption: after closeout, assert working-tree == HEAD"
  wait_for_marker "reposition boundary signal sent" || warn "no post-commit reposition marker seen before timeout"
  # The bug = the working tree drifting from HEAD post-commit. Assert tree==HEAD.
  if git -C "$REPO" rev-parse --verify HEAD:"${doc#"$REPO"/}" >/dev/null 2>&1; then
    head_blob="$(git -C "$REPO" show HEAD:"${doc#"$REPO"/}" 2>/dev/null || true)"
    tree_blob="$(cat "$doc" 2>/dev/null || true)"
    if [[ "$head_blob" == "$tree_blob" ]]; then
      log "PASS [postcommit-ipc-worktree-corruption]: working tree == HEAD"
    else
      warn "FAIL [postcommit-ipc-worktree-corruption]: working tree DRIFTED from HEAD post-commit"
      return 1
    fi
  else
    warn "scratch doc not yet committed; run a closeout cycle first"
    return 1
  fi
}

case_saevon() {
  local doc base wid
  doc="$(ensure_scratch_doc saevon)"; base="$(basename "$doc")"
  wid="$(require_window "$base")"
  log "#saevon: requires EARLY_ACK_ENABLED=true + cargo build --release + agent-doc lib-install first"
  log "         expect '[ipc-socket] early-ack pending emitted before apply' with NO false-success / NO false ack-timeout"
  wait_for_marker "ipc.*apply\|reposition boundary signal sent" || warn "no IPC-apply marker before timeout; injecting edit anyway"
  type_into_scratch "$wid" "$base" "early-ack load edit"
  assert_marker saevon "early-ack pending emitted before apply" \
    && assert_no_marker saevon "ack-timeout"
}

case_tmux_switch() {
  log "#tmux-switch-lag: the offline-assertable half (pane move-before-select ordering)"
  log "  is covered deterministically in SimWorld; this live recipe only confirms no"
  log "  intermediate stash frame on a doc-to-doc switch. Drive the switch via the editor"
  log "  tab/tmux and watch for a stash-layout flash. Frame capture (scrot/xdotool) is"
  log "  optional and environment-specific; not asserted here."
  warn "tmux-switch live frame assertion is manual/observational — see plan per-item recipe"
}

case_lvbatch_markers() {
  local olog; olog="$(ops_log)"
  log "#lvbatch markers — grepping ops.log for code-complete live markers"
  local ok=0
  assert_marker lvbatch:f5d2-pcp6 "live_buffer_classify" || ok=1
  # #4wxr / #9adk / #saev are driven by their own live actions; presence here is informational.
  for m in "visible_write_live_buffer_matches_disk" "visible_write_deferred_current_changed"; do
    if [[ -f "$olog" ]] && grep -q -- "$m" "$olog" 2>/dev/null; then
      log "INFO [lvbatch]: ops.log has '$m'"
    fi
  done
  return $ok
}

run_case() {
  check_env
  assert_fresh_session
  case "$CASE" in
    exch-intermix)        case_exch_intermix ;;
    postcommit-worktree)  case_postcommit_worktree ;;
    saevon)               case_saevon ;;
    tmux-switch)          case_tmux_switch ;;
    lvbatch-markers)      case_lvbatch_markers ;;
    *) die "unknown case '$CASE' (try: check-env | list | exch-intermix | postcommit-worktree | saevon | tmux-switch | lvbatch-markers)" ;;
  esac
}

main() {
  parse_args "$@"
  case "${CASE:-}" in
    ""|list)
      sed -n '2,40p' "$0"
      ;;
    check-env)
      check_env
      ;;
    *)
      run_case
      ;;
  esac
}

main "$@"
