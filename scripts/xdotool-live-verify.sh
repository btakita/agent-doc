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
#   1. Types ONLY into a throwaway scratch doc tmp/live-repro/xdotool-<case>.md,
#      never the real working doc.
#   2. Focus-guards getactivewindow/getwindowname before every type/key; aborts on
#      any mismatch so a stray keystroke cannot land in the wrong window.
#   3. Warns if launched from a degraded/restart-heavy session (verify from a FRESH
#      session — this is where #postcommit-ipc-worktree-corruption / #saevon false
#      successes hide).
#   4. Times keystrokes by an ops.log IPC-apply marker, not a fixed sleep.
#   5. Opens the scratch doc in the ALREADY-RUNNING IDE itself (via $AGENT_DOC_IDE_LAUNCHER
#      or `idea`) so no human has to open a file first; the open is refused for any
#      path outside tmp/live-repro/. It still never launches a cold IDE.
#
# Usage:
#   scripts/xdotool-live-verify.sh check-env
#   scripts/xdotool-live-verify.sh list
#   scripts/xdotool-live-verify.sh <case> [--repo <dir>] [--dry-run] [--timeout <sec>]
#
# Cases: exch-intermix | postcommit-worktree | saevon | captured-splice | tmux-switch
#        | lvbatch-markers
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
# Placeholder window id used only by --dry-run when no live editor is attached.
DRYRUN_WID="dry-run-no-window"

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

# The binary logs a document's events under ITS project root: the nearest
# ancestor holding a `.agent-doc/` directory. A scratch doc lives inside
# `$REPO/tmp/live-repro/`. Scratch docs used to live under
# `$REPO/.agent-doc/live-repro/`, which made `$REPO/.agent-doc` the nearest
# ancestor with a `.agent-doc/` child once any write created
# `$REPO/.agent-doc/.agent-doc/`: every scratch doc became its own nested project
# with its own controller, lease and log, and the recipe waited on the wrong log.
# Resolve the log exactly as the binary does, starting from the scratch doc.
OPS_LOG_DOC=""
ops_log() {
  local dir
  dir="$(dirname "${OPS_LOG_DOC:-$REPO/x}")"
  while [[ "$dir" != "/" ]]; do
    [[ -d "$dir/.agent-doc" ]] && { printf '%s/.agent-doc/logs/ops.log' "$dir"; return; }
    dir="$(dirname "$dir")"
  done
  printf '%s/.agent-doc/logs/ops.log' "$REPO"
}
scratch_doc(){ printf '%s/tmp/live-repro/xdotool-%s.md' "$REPO" "$1"; }

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
# Match the scratch doc by its absolute path whenever it is known. The IDE title
# ends with the active file's absolute path; a basename match accepted a stale
# tab of an older scratch doc with the same filename and typed into it.
title_is_scratch() {
  local title="$1" base="$2" doc="${3:-}"
  if [[ -n "$doc" ]]; then
    [[ "$title" == *"$doc" ]]
  else
    [[ "$title" == *"$base"* ]]
  fi
}

resolve_window() {
  local base="$1" doc="${2:-}" wid title
  for wid in $(xdotool search --name "$base" 2>/dev/null || true); do
    title="$(xdotool getwindowname "$wid" 2>/dev/null || true)"
    if title_is_scratch "$title" "$base" "$doc"; then
      printf '%s' "$wid"
      return 0
    fi
  done
  return 1
}

# Focus-guard: the active window must be the intended scratch editor before any
# type/key. Aborts otherwise so a stray keystroke cannot corrupt real work.
#
# `#activateinstalledjetbrai`: an IDE window's TITLE follows its selected editor
# tab, so a concurrent agent-doc session switching tabs in the SAME IDE moves this
# guard's title off the scratch doc without the window ever losing focus. During a
# queue drain that happens every 1-3s, which made a single-shot guard abort the
# recipe as a matter of course — correctly refusing, but never able to finish.
# Losing the tab is recoverable and the recovery is the one the harness already
# owns: ask the running IDE to open the scratch doc again, which re-selects its
# tab. So re-acquire on a bounded deadline and abort only if the scratch doc
# cannot be brought back — the refusal stays exactly as strict, it just stops
# being permanent.
focus_guard() {
  local wid="$1" base="$2" doc="${3:-}" active active_title deadline reopened=0
  if [[ "$DRY_RUN" == 1 && "$wid" == "$DRYRUN_WID" ]]; then
    log "[dry-run] would focus-guard '$base' before typing"
    return 0
  fi
  deadline=$(( $(date +%s) + TIMEOUT ))
  while :; do
    active="$(xdotool getactivewindow 2>/dev/null || true)"
    [[ -n "$active" ]] || die "focus-guard: no active window"
    if [[ "$active" != "$wid" ]]; then
      xdotool windowactivate --sync "$wid" 2>/dev/null || die "focus-guard: cannot activate scratch window $wid"
      active="$(xdotool getactivewindow 2>/dev/null || true)"
    fi
    active_title="$(xdotool getwindowname "$active" 2>/dev/null || true)"
    title_is_scratch "$active_title" "$base" "$doc" && return 0
    (( $(date +%s) >= deadline )) \
      && die "focus-guard: active window '$active_title' is not the scratch doc '$base' after ${TIMEOUT}s — ABORT (would corrupt real work)"
    # The window is focused but showing another document: a concurrent session
    # selected a different tab. Re-open the scratch doc to select it back. Only
    # the IDE can do that; xdotool cannot address an editor tab.
    if [[ -n "$doc" && "$reopened" == 0 ]]; then
      log "focus-guard: '$active_title' took the tab — asking the IDE to re-open '$base'"
      reopened=1
      open_scratch_in_ide "$doc" "$base" >/dev/null 2>&1 || true
    fi
    sleep 0.2
  done
}

# --- marker-timed typing ----------------------------------------------------
# Wait for an IPC-apply sentinel to appear in ops.log (timing by marker, not sleep),
# then return so the caller can inject the concurrent edit at the drift window.
# ops.log byte offset captured before an action; wait_for_marker only counts
# lines written after it. Matching the whole file returned instantly on any
# historical line, so "timed by a marker" timed nothing.
MARK_OFFSET=0
mark_ops_log() {
  local olog; olog="$(ops_log)"
  MARK_OFFSET="$( [[ -f "$olog" ]] && stat -c %s "$olog" || echo 0 )"
}

# wait_for_marker <marker> [doc-stem]: with a stem, only lines carrying
# `doc=<stem>` count, so another document's receipt cannot satisfy the wait.
wait_for_marker() {
  local marker="$1" stem="${2:-}" olog deadline now
  olog="$(ops_log)"
  if [[ -n "$stem" && "$DRY_RUN" != 1 ]]; then
    deadline=$(( $(date +%s) + TIMEOUT ))
    while :; do
      if [[ -f "$olog" ]] && tail -c +"$(( MARK_OFFSET + 1 ))" "$olog" 2>/dev/null \
          | grep -F -- "$marker" | grep -qF -- "doc=$stem "; then
        return 0
      fi
      now=$(date +%s)
      (( now >= deadline )) && return 1
      sleep 0.1
    done
  fi
  # --dry-run types nothing, so no NEW marker can arrive; poll once instead of
  # burning the whole timeout. Cases that assert pre-existing markers still match.
  if [[ "$DRY_RUN" == 1 ]]; then
    [[ -f "$olog" ]] && grep -q -- "$marker" "$olog" 2>/dev/null
    return $?
  fi
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
  local wid="$1" base="$2" text="$3" doc="${4:-}"
  focus_guard "$wid" "$base" "$doc"
  if [[ "$DRY_RUN" == 1 ]]; then
    log "[dry-run] would type into $wid ($base): '$text'"
    return 0
  fi
  # Type through XTEST at the proven-active window, NOT `type --window`.
  # `--window` routes through XSendEvent, which stamps every event with
  # `send_event=True`; the JetBrains AWT toolkit drops those, so the old path
  # "typed" into the IDE while nothing ever reached the editor (zero doc-scoped
  # ops.log events for any xdotool scratch doc, ever). XTEST goes to whatever
  # holds focus, so focus is re-proven before every short chunk: a focus change
  # can misdirect at most one chunk, and the guard aborts before the next.
  local chunk rest="$text"
  # Type at the end of the document: the caret opens at offset 0, and text typed
  # there lands above the frontmatter and unmakes the session document.
  focus_guard "$wid" "$base" "$doc"
  xdotool key --clearmodifiers ctrl+End Return
  while [[ -n "$rest" ]]; do
    chunk="${rest:0:8}"; rest="${rest:8}"
    focus_guard "$wid" "$base" "$doc"
    xdotool type --delay 40 -- "$chunk"
  done
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
<!-- agent:boundary:00000000:scratch -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->
EOF
    log "created scratch doc $doc"
  fi
  printf '%s' "$doc"
}

# --- IDE open (removes the last human prerequisite) --------------------------
# `#activateinstalledjetbrai`: this harness used to die telling a human to open the
# scratch doc in IntelliJ, and that one step is what kept the item operator-gated.
# The running IDE can be asked to open a file by its own launcher, so the harness
# does it itself. Only ever the throwaway scratch doc — asserted below, because an
# IDE-open of a real working doc is the same class of mistake the focus guard exists
# to prevent.
ide_launcher() {
  local c
  for c in "${AGENT_DOC_IDE_LAUNCHER:-}" idea; do
    [[ -n "$c" ]] || continue
    command -v "$c" >/dev/null 2>&1 && { printf '%s' "$c"; return 0; }
  done
  return 1
}

open_scratch_in_ide() {
  local doc="$1" base="$2" launcher deadline
  # Safety: refuse to hand anything but a live-repro scratch doc to the IDE.
  [[ "$doc" == "$REPO/tmp/live-repro/"* && "$doc" != *"/.agent-doc/"* ]] \
    || die "refusing to IDE-open '$doc' — only tmp/live-repro/ scratch docs may be opened automatically"
  if ! launcher="$(ide_launcher)"; then
    warn "no IDE launcher found (tried \$AGENT_DOC_IDE_LAUNCHER, idea)"
    return 1
  fi
  if [[ "$DRY_RUN" == 1 ]]; then
    log "[dry-run] would ask the running IDE to open the scratch doc: $launcher $doc"
    return 1
  fi
  log "no window for '$base' yet — asking the running IDE to open it ($launcher)"
  "$launcher" "$doc" >/dev/null 2>&1 || true
  # Poll for the window instead of sleeping: an IDE open is slow and variable, so a
  # fixed sleep would either flake or pad every run.
  deadline=$(( $(date +%s) + TIMEOUT ))
  while :; do
    resolve_window "$base" "$doc" >/dev/null 2>&1 && return 0
    (( $(date +%s) >= deadline )) && return 1
    sleep 0.2
  done
}

require_window() {
  local base="$1" doc="${2:-}" wid
  if ! wid="$(resolve_window "$base" "$doc")"; then
    if [[ -n "$doc" ]] && open_scratch_in_ide "$doc" "$base"; then
      wid="$(resolve_window "$base" "$doc")" \
        || die "the IDE accepted the open but no window titled '*$base*' resolved within ${TIMEOUT}s"
    elif [[ "$DRY_RUN" == 1 ]]; then
      # --dry-run exists to review the recipe without touching a live desktop, so it
      # must not require one. Hand back a sentinel the focus guard recognises.
      warn "[dry-run] no live window for '$base'; printing the remaining recipe against a placeholder window"
      printf '%s' "$DRYRUN_WID"
      return 0
    else
      die "no live editor window titled '*$base*' and it could not be opened automatically — open the scratch doc in the IDE, or set AGENT_DOC_IDE_LAUNCHER to a launcher that opens a file in the RUNNING instance"
    fi
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
  wid="$(require_window "$base" "$doc")"
  log "#exch-intermix-verify: type a mid-finalize edit, expect live_prompt_drift_auto_recovered"
  wait_for_marker "ipc.*apply\|reposition boundary signal sent" || warn "no IPC-apply marker seen before timeout; injecting edit anyway"
  type_into_scratch "$wid" "$base" "mid-finalize concurrent edit" "$doc"
  assert_marker exch-intermix-verify "live_prompt_drift_auto_recovered" \
    && assert_no_marker exch-intermix-verify "looks like a manual cleanup"
}

case_postcommit_worktree() {
  local doc base wid head_blob tree_blob
  doc="$(ensure_scratch_doc postcommit-worktree)"; base="$(basename "$doc")"
  wid="$(require_window "$base" "$doc")"
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
  wid="$(require_window "$base" "$doc")"
  log "#saevon: requires EARLY_ACK_ENABLED=true + cargo build --release + agent-doc lib-install first"
  log "         expect '[ipc-socket] early-ack pending emitted before apply' with NO false-success / NO false ack-timeout"
  wait_for_marker "ipc.*apply\|reposition boundary signal sent" || warn "no IPC-apply marker before timeout; injecting edit anyway"
  type_into_scratch "$wid" "$base" "early-ack load edit" "$doc"
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

# `#activateinstalledjetbrai` — the gate that used to need a human eyeball.
#
# The property is: a captured local editor edit recovers across an
# INDEPENDENTLY ADVANCED canonical response. Both halves have to be driven, in
# order, and the receipt evaluation belongs to the binary
# (`agent-doc verify-captured-splice-recovery`), not to greps here:
#
#   1. type an operator edit into the scratch doc (real keystrokes → the plugin's
#      documentChanged listener → op capture);
#   2. run a response cycle so the canonical text advances on its own;
#   3. type a second operator edit, so a capture sits BETWEEN two splice
#      recoveries that observed different canonical text;
#   4. let the binary decide whether the receipts prove the property.
#
# Step 2 is the part a bare "type and grep" recipe skips, and skipping it is why
# earlier evidence sweeps found splice recoveries with nothing to recover across.
case_captured_splice() {
  local doc base wid rel
  doc="$(ensure_scratch_doc captured-splice)"; base="$(basename "$doc")"
  OPS_LOG_DOC="$doc"
  log "receipts for this doc are read from $(ops_log)"
  rel="${doc#"$REPO"/}"
  assert_scratch_authority "$rel"
  wid="$(require_window "$base" "$doc")"
  # Opening the doc can bind it to a pane for the first time; re-read before typing.
  assert_scratch_authority "$rel"
  log "#activateinstalledjetbrai: operator edit → independent response advance → operator edit"

  mark_ops_log
  type_into_scratch "$wid" "$base" "operator edit one before the advance" "$doc"
  # Time on THIS document's capture receipt, not a sleep and not any historical
  # line: no fresh receipt means the reporter chain never ran and the rest of the
  # recipe would prove nothing.
  # --dry-run types nothing, so a fresh receipt can never arrive: name the wait
  # instead of dying on it, so the dry run prints the whole recipe offline.
  if [[ "$DRY_RUN" == 1 ]]; then
    log "[dry-run] would wait for a fresh editor_op_capture_proof for ${base%.md}"
  else
    wait_for_marker "editor_op_capture_proof" "${base%.md}" \
    || die "no fresh editor_op_capture_proof for ${base%.md} within ${TIMEOUT}s — the keystrokes did not reach the editor, or the epoch was refused (agent-doc verify-op-capture $rel names which)"
  fi

  if [[ "$DRY_RUN" == 1 ]]; then
    log "[dry-run] would advance the canonical response via: agent-doc write --commit $rel (in pane ${SCRATCH_OWNER_PANE:-self})"
  else
    log "advancing the canonical response independently of the editor"
    advance_in_owner_pane "$rel" \
      || warn "response advance did not complete; the verifier will report an unadvanced canonical text"
  fi

  type_into_scratch "$wid" "$base" "operator edit two after the advance" "$doc"

  if [[ "$DRY_RUN" == 1 ]]; then
    log "[dry-run] would assert: agent-doc verify-captured-splice-recovery $rel"
    return 0
  fi
  if (cd "$REPO" && agent-doc verify-captured-splice-recovery "$rel"); then
    log "PASS [activateinstalledjetbrai]: receipts prove recovery across an independent advance"
    return 0
  fi
  warn "FAIL [activateinstalledjetbrai]: see the verifier's diagnosis above — it names which link is missing"
  return 1
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

# --- pane-authority guard ---------------------------------------------------
# `#activateinstalledjetbrai`: the recipe's canonical-advance step runs
# `agent-doc write --commit`, which is refused from a non-owning pane
# ("pane execution authority rejected before mutation"). That refusal used to
# arrive AFTER keystrokes had already been injected into a live desktop — a live
# side effect for a recipe that could never complete. Fail closed BEFORE typing.
#
# Read-only and tmux-native on purpose: `agent-doc route` dispatches rather than
# reports, so there is no read-only authority query to call here. An agent-doc pane
# whose cwd is the target repo owns that repo's controller.
# The scratch doc is a session document in its own right: once any IDE opened
# it, pane layout bound it to its own pane, and `write --commit` is refused from
# every other pane. Ownership is per DOCUMENT, so read the scratch doc's actor.
SCRATCH_OWNER_PANE=""
scratch_owner_pane() {
  local rel="$1"
  (cd "$REPO" && agent-doc session status "$rel" 2>/dev/null) \
    | awk '/^actor:/ { for (i=1;i<=NF;i++) if ($i ~ /^pane=%/) { sub("pane=","",$i); print $i; exit } }' \
    || true  # a doc with no agent_doc_session has no owner; never abort the caller
}

# An owner pane can host the advance only when it is an idle shell: typing a
# command into a live agent pane would be an operator prompt, not a write.
owner_pane_is_idle_shell() {
  local pane="$1" cmd
  cmd="$(tmux display-message -p -t "$pane" '#{pane_current_command}' 2>/dev/null)" || return 1
  [[ "$cmd" =~ ^(zsh|bash|sh|fish)$ ]]
}

assert_scratch_authority() {
  local rel="$1" self="${TMUX_PANE:-}"
  command -v tmux >/dev/null 2>&1 || return 0
  SCRATCH_OWNER_PANE="$(scratch_owner_pane "$rel")"
  [[ -z "$SCRATCH_OWNER_PANE" || "$SCRATCH_OWNER_PANE" == "$self" ]] && { SCRATCH_OWNER_PANE=""; return 0; }
  if owner_pane_is_idle_shell "$SCRATCH_OWNER_PANE"; then
    log "scratch doc is owned by idle shell pane $SCRATCH_OWNER_PANE; the canonical advance will run there"
    return 0
  fi
  [[ "$DRY_RUN" == 1 ]] && { warn "[dry-run] scratch owner $SCRATCH_OWNER_PANE is not an idle shell"; return 0; }
  die "scratch doc is owned by pane $SCRATCH_OWNER_PANE, which is not an idle shell — refusing to inject keystrokes for a recipe whose advance cannot run"
}

# Run the advance in the owning pane and wait for its exit status via a sentinel.
advance_in_owner_pane() {
  local rel="$1" sentinel rc deadline body
  # The advance must carry a response: `write --commit` with empty stdin is
  # refused ("empty response — nothing to write"), so the canonical text never
  # moved and the verifier could only ever report an unadvanced document.
  body="$(mktemp "${TMPDIR:-/tmp}/xdotool-advance-body.XXXXXX")"
  printf '<!-- patch:exchange -->\n### Re: captured-splice advance — xdotool\n\nCanonical response advanced independently of the editor at %s.\n<!-- /patch:exchange -->\n' \
    "$(date -u +%FT%TZ)" > "$body"
  if [[ -z "$SCRATCH_OWNER_PANE" ]]; then
    (cd "$REPO" && agent-doc write --commit "$rel" < "$body")
    rc=$?; rm -f "$body"; return $rc
  fi
  sentinel="$(mktemp -u "${TMPDIR:-/tmp}/xdotool-advance.XXXXXX")"
  tmux send-keys -t "$SCRATCH_OWNER_PANE" -l -- \
    "cd $(printf '%q' "$REPO") && agent-doc write --commit $(printf '%q' "$rel") < $(printf '%q' "$body"); echo \$? > $(printf '%q' "$sentinel")"
  tmux send-keys -t "$SCRATCH_OWNER_PANE" Enter
  deadline=$(( $(date +%s) + TIMEOUT * 4 ))
  until [[ -s "$sentinel" ]]; do
    (( $(date +%s) >= deadline )) && { warn "advance in $SCRATCH_OWNER_PANE did not finish"; return 1; }
    sleep 0.2
  done
  rc="$(cat "$sentinel")"; rm -f "$sentinel" "$body"
  log "advance in $SCRATCH_OWNER_PANE exited $rc"
  [[ "$rc" == 0 ]]
}

assert_pane_authority() {
  command -v tmux >/dev/null 2>&1 || return 0
  local self owners
  self="${TMUX_PANE:-}"
  owners="$(tmux list-panes -a -F '#{pane_id} #{pane_current_command} #{pane_current_path}' 2>/dev/null \
    | awk -v repo="$REPO" -v self="$self" '$2 == "agent-doc" && $3 == repo && $1 != self { print $1 }' \
    | tr '\n' ' ')"
  [[ -n "${owners// /}" ]] || return 0
  warn "another agent-doc pane owns this repo's controller: ${owners% }"
  warn "the canonical-advance step (agent-doc write --commit) is refused from a non-owning pane,"
  warn "so this recipe cannot complete here — drive it from the owning pane, or point --repo at a"
  warn "repo root this pane owns."
  [[ "$DRY_RUN" == 1 ]] \
    || die "refusing to inject keystrokes for a recipe that cannot complete from this pane (owning pane: ${owners% })"
}

run_case() {
  check_env
  assert_fresh_session
  assert_pane_authority
  case "$CASE" in
    exch-intermix)        case_exch_intermix ;;
    postcommit-worktree)  case_postcommit_worktree ;;
    saevon)               case_saevon ;;
    captured-splice)      case_captured_splice ;;
    tmux-switch)          case_tmux_switch ;;
    lvbatch-markers)      case_lvbatch_markers ;;
    *) die "unknown case '$CASE' (try: check-env | list | exch-intermix | postcommit-worktree | saevon | captured-splice | tmux-switch | lvbatch-markers)" ;;
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
