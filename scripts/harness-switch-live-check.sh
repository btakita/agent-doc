#!/usr/bin/env bash
# harness-switch-live-check.sh — scaffold a DISPOSABLE session document for live
# authoritative-harness-switch checks, print the one-screen operator procedure, and
# verify the receipts (#hswdisposabledoc).
#
# Why a throwaway document: the check requires editing a frontmatter `agent:` line
# and leaving the buffer UNSAVED while a live actor of the old harness owns the pane.
# Doing that to a real session document puts an unsaved operator edit in front of the
# CP/CRDT authority path for no reason.
#
# Deterministic behavior stays in the binary (CLAUDE.md): `agent-doc init` creates the
# document and `agent-doc verify-harness-switch` counts the receipts. This script only
# scaffolds, prints the procedure, and calls them. It deliberately does NOT grep
# ops.log itself — the invariant is "exactly ONE spawn per switch", which a shell
# `grep -q` cannot express and which inverts on a match.
#
# Usage:
#   scripts/harness-switch-live-check.sh scaffold [--agent <harness>] [--name <slug>]
#   scripts/harness-switch-live-check.sh steps   [--name <slug>]
#   scripts/harness-switch-live-check.sh verify  [--name <slug>] [--expect-switches <n>]
#   scripts/harness-switch-live-check.sh clean   [--name <slug>]
#
# Related: route_sim_consecutive_harness_switches_spawn_exactly_once_each_with_no_storm
# guards the same invariant offline (#hswspawncount); prefer it unless the check
# genuinely needs a live editor and pane.

set -euo pipefail

ACTION="${1:-steps}"
shift || true

AGENT="codex"
NAME="harness-switch"
SWITCHES=""

while [ $# -gt 0 ]; do
  case "$1" in
    --agent) AGENT="${2:?--agent needs a value}"; shift 2 ;;
    --name) NAME="${2:?--name needs a value}"; shift 2 ;;
    --expect-switches) SWITCHES="${2:?--expect-switches needs a value}"; shift 2 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done

repo_root() {
  git rev-parse --show-toplevel 2>/dev/null || pwd
}

ROOT="$(repo_root)"
DOC_DIR="$ROOT/.agent-doc/live-repro"
DOC="$DOC_DIR/$NAME.md"

# The switch target: whatever the scaffolded harness is NOT. Keeping this derived
# means `steps` cannot print a procedure that contradicts the document on disk.
switch_target() {
  local from="$1"
  case "$from" in
    codex) echo "claude" ;;
    *) echo "codex" ;;
  esac
}

scaffolded_agent() {
  [ -f "$DOC" ] || return 1
  sed -n 's/^agent:[[:space:]]*\([^[:space:]]*\).*/\1/p' "$DOC" | head -1
}

print_steps() {
  local from to
  from="$(scaffolded_agent 2>/dev/null || echo "$AGENT")"
  to="$(switch_target "$from")"
  cat <<STEPS
─── live harness-switch check ──────────────────────────────────────────────────
document   $DOC
scaffolded  agent: $from        switch to     agent: $to

1. Open the document and start its session so a live $from actor owns the pane:
     agent-doc $DOC

2. Wait for a dispatch-ready prompt. The gate only fires at a QUIET boundary, so a
   busy turn or a paused queue holds the switch (that is a hold, not a failure).

3. In the editor, change ONE line of frontmatter and DO NOT SAVE:
     agent: $from   ->   agent: $to
   Unsaved is the point: the switch must be observed through the editor buffer's
   authority, not through a disk write.

4. Wait for the next quiet prompt boundary, then verify:
     agent-doc verify-harness-switch $DOC --expect-switches 1

   Expected receipts, in order:
     harness_change_detected old=$from new=$to gate=Restart
     agent_restart_triggered old=$from new=$to action=request_fresh_restart
     agent_restart_performed old_harness=$from new_harness=$to action=spawn_fresh_harness

   Exactly ONE spawn. Two is a respawn storm; --expect-switches is what makes the command
   able to say so, so do not omit it.

   gate=WaitForBoundary means no quiet boundary was reached yet — wait and re-verify.
   gate=None means agent_change_restart is disabled for this document.

5. When done:
     scripts/harness-switch-live-check.sh clean --name $NAME
────────────────────────────────────────────────────────────────────────────────
STEPS
}

case "$ACTION" in
  scaffold)
    mkdir -p "$DOC_DIR"
    if [ -f "$DOC" ]; then
      echo "already scaffolded: $DOC" >&2
    else
      agent-doc init "$DOC" "harness switch live check" --agent "$AGENT" --mode template
      echo "scaffolded $DOC (agent: $AGENT)"
    fi
    print_steps
    ;;
  steps)
    if [ ! -f "$DOC" ]; then
      echo "no scaffolded document at $DOC — run: scripts/harness-switch-live-check.sh scaffold" >&2
      exit 1
    fi
    print_steps
    ;;
  verify)
    if [ ! -f "$DOC" ]; then
      echo "no scaffolded document at $DOC — run: scripts/harness-switch-live-check.sh scaffold" >&2
      exit 1
    fi
    if [ -n "$SWITCHES" ]; then
      agent-doc verify-harness-switch "$DOC" --expect-switches "$SWITCHES"
    else
      echo "note: without --expect-switches this only checks the receipt trail; a respawn storm passes" >&2
      agent-doc verify-harness-switch "$DOC"
    fi
    ;;
  clean)
    if [ -f "$DOC" ]; then
      rm -f "$DOC"
      echo "removed $DOC"
    else
      echo "nothing to remove at $DOC"
    fi
    ;;
  *)
    echo "usage: $0 {scaffold|steps|verify|clean} [--agent <harness>] [--name <slug>] [--expect-switches <n>]" >&2
    exit 2
    ;;
esac
