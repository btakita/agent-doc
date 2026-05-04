# Plan — detect bare SSH socket EPERM as required-SSH capability drift

**Pending:** `#ssheperm`
**Status:** Draft

## Problem

May 3, 2026 user report:

- a Codex-backed `agent-doc` session attempted to SSH into the production server
- the Codex transcript surfaced `socket: Operation not permitted`
- the session did not get classified into the existing required-SSH capability-drift recovery path

This is a narrower follow-up to `plan-codex-ssh-server-access-contract.md`, not a duplicate of it.

- the earlier work added `required_ssh_targets` plus resumed-session fresh-retry coverage for target-specific failures like `ssh: connect to host 50.28.2.199 port 22: Operation not permitted`
- this new report suggests the live transcript can still emit a shorter SSH client error string that bypasses the current detector and leaks through as a raw task failure

## Desired Behavior

1. A document that declares `required_ssh_targets` must treat `socket: Operation not permitted` from an SSH attempt as a required-SSH capability failure, not as an ordinary failed shell command.
2. If the failing session is a resumed Codex thread, `agent-doc` should retry once with fresh `codex exec` before trusting the old session again.
3. If the fresh retry still cannot prove the required SSH capability, `agent-doc` should fail closed with a clear blocker message.
4. The detector must not accidentally classify unrelated local-socket/CDP `Operation not permitted` text as an SSH failure.

## Likely Root Cause Surface

- `src/agent-doc/src/agent/codex.rs`
  - `transcript_has_required_ssh_failure(...)`
  - target-term gating that currently expects the failing transcript line to still mention `monsterrodholders-server` or `50.28.2.199`
  - blocking and streaming resume-retry paths that only trigger when `required_ssh_failure` is detected
- command-output parsing
  - a command execution item may emit only `socket: Operation not permitted` even when the command was `ssh ...`
  - current detection likely has insufficient surrounding-command/context awareness

## Implementation Steps

- [ ] Reproduce the miss with a focused Codex backend regression:
  - resumed session
  - `required_ssh_targets: [monsterrodholders-server]`
  - command execution output containing only `socket: Operation not permitted`
  - verify the current parser does not trigger the fresh retry
- [ ] Audit the required-SSH detector for both blocking and streaming paths:
  - how `aggregated_output` is scanned
  - whether command text / sibling output / surrounding lines are available
  - whether the detector can safely infer "this EPERM came from SSH" without the hostname string
- [ ] Tighten the classifier so bare socket EPERM still counts when SSH context is proven:
  - direct command string mentions `ssh`
  - or adjacent output lines from the same command execution prove SSH
  - while explicitly excluding the known localhost/CDP drift family
- [ ] Ensure the existing recovery contract still holds:
  - resumed thread gets one fresh retry
  - fresh success replaces the stale resume thread
  - repeated failure returns a fail-closed required-SSH error
- [ ] Improve diagnostics so the surfaced error says that the session lost required SSH capability rather than only echoing the raw socket error
- [ ] Add regression coverage for:
  - bare `socket: Operation not permitted` on required SSH
  - target-specific SSH EPERM still works
  - CDP/local-browser EPERM does not get misclassified as SSH

## Files Likely To Change

- `src/agent-doc/src/agent/codex.rs`
- `src/agent-doc/specs/05-agent-backend.md`
- `src/agent-doc/VERSIONS.md`

## Exit Criteria

- a resumed Codex session that needs `monsterrodholders-server` access no longer leaks bare `socket: Operation not permitted` through the normal task path
- required-SSH capability drift is detected for both hostname-bearing and bare-socket SSH EPERM outputs
- the detector still keeps SSH failures separate from localhost/CDP capability drift
- regression tests cover the exact transcript shape from this report

