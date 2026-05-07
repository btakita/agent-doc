# Plan — stop `agent-doc` from preventing later SSH connections

**Pending:** `#sshcut` in `tasks/agent-doc/agent-doc-bugs2.md`

## Problem

Recent `monsterrodholders` sessions reported that after running `agent-doc`, later SSH work was no longer reliably available. The user-facing symptom is broader than the existing required-SSH drift items:

- the issue is not just "this Codex child cannot prove required SSH"
- the report is that `agent-doc` itself may be leaving the environment in a state where later SSH connections fail or are blocked

That distinction matters. If `agent-doc` only lacks SSH inside one managed Codex child, the fix belongs in required-capability probing and fresh-session retry logic. If `agent-doc` mutates or poisons the operator's wider SSH environment, the bug is more severe and the fix belongs in process/env isolation or teardown.

## Goal

Prove which layer is actually breaking, then fail closed before `agent-doc` can leave SSH in a worse state than it found it:

1. distinguish "child session lost required SSH" from "later SSH on the host is now broken after `agent-doc` ran"
2. identify whether the breakage is caused by environment inheritance, config-path mutation, permission side effects, long-lived helper state, or sandbox/policy drift
3. add regression coverage and diagnostics so future SSH-dependent docs surface the right blocker immediately

## Investigation

- [ ] Capture a fresh before/after repro around one SSH-dependent doc such as `src/boost-client/tasks/monsterrodholders.md`:
  - prove whether plain `ssh <target> true` works before `agent-doc`
  - run the exact `agent-doc` cycle that triggers the complaint
  - prove whether the same SSH command now fails outside the child session, and record the exact error text
- [ ] Separate the failure surface:
  - `agent-doc` parent process only
  - spawned Codex child only
  - later shell sessions / later tmux panes after the cycle
- [ ] Audit for state that can leak across the session boundary:
  - exported env vars and shell snapshot restoration
  - SSH-related flags or config overrides passed to child commands
  - writable temp files, control sockets, known_hosts/config includes, or permission changes touched during the cycle
  - supervisor or hook processes that outlive the intended turn and keep stale capability assumptions alive
- [ ] Compare against the existing required-SSH machinery:
  - `required_ssh_targets`
  - resumed-session fresh-retry logic
  - fail-closed handling for `socket: Operation not permitted`, DNS failure, and config-readability errors
  - identify where the current logic incorrectly treats a broader SSH outage as a document-local failure or vice versa

## Fix Boundary

- [ ] If the repro is child-local only:
  - tighten capability probing / retry so `agent-doc` fails closed without implying that SSH is generally available
- [ ] If the repro persists in later shells after `agent-doc` exits:
  - isolate the mutation source
  - stop writing or exporting the offending SSH-related state
  - add teardown/cleanup for any helper artifacts or environment overrides
- [ ] In both cases:
  - surface a precise blocker message that says whether SSH is unavailable only inside the managed session or also appears broken for later operator shells

## Verification

- [ ] Add a regression or harness-level proof for the identified class:
  - child-session-only SSH capability loss
  - cross-session SSH breakage after `agent-doc`
  - successful cleanup path where a completed `agent-doc` run leaves later SSH commands unaffected
- [ ] Re-run the concrete `monsterrodholders` SSH-dependent workflow and confirm:
  - `agent-doc` either preserves SSH access or fails closed before work starts
  - later plain SSH attempts still behave exactly as they did before the run

## Exit Criteria

- `agent-doc` no longer leaves the operator wondering whether it broke SSH globally or only inside one managed child
- SSH-dependent docs fail with the correct scope-specific diagnostic
- a completed or failed `agent-doc` run does not poison later SSH connections
