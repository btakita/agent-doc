# Plan: required SSH guard must not match historical capture text

## Problem

The Codex required-SSH guard can fail a fresh `agent-doc <file>` run because a
child command prints historical text that contains both the required target and
an old SSH failure.

Evidence:

- Document: `src/boost-client/tasks/monsterrodholders.md`
- User-visible failure:

  ```text
  Error: required SSH capability failed for target(s) monsterrodholders-server:
  .agent-doc/captures/.../cycle-1777858230777.json:16:
    "response_body": "... socket: Operation not permitted ..."
  ```

- The reported detail is not the fresh SSH probe. It is a line from an old
  durable capture JSON file whose response body mentioned `socket: Operation not
  permitted`.
- Code path: `src/agent-doc/src/agent/codex.rs`
  `command_execution_required_ssh_failure()` first scans `aggregated_output` for
  required target terms plus SSH failure text before proving the command itself
  was an SSH command. That lets commands such as `rg` over `.agent-doc/captures`
  be misclassified as live required-SSH failures.

This is different from legitimate `socket: Operation not permitted` capability
loss. The guard should still fail closed when an actual `ssh
monsterrodholders-server ...` command hits EPERM. It should not fail because the
agent read archived text containing that error.

## Desired Behavior

1. Required-SSH failure detection for `command_execution` events should be tied
   to an SSH command/probe context, not arbitrary command output.
2. Historical artifacts under `.agent-doc/captures`, logs, docs, or grep output
   should not trigger the live capability guard.
3. Diagnostics should distinguish:
   - pre-dispatch SSH proof failure
   - resumed-session SSH drift from an actual SSH command
   - archived text mentioning an old SSH failure
4. A fresh failure should report the actual command/probe and stderr/stdout, not
   a matching file path from unrelated search output.

## Investigation Steps

- [ ] Reproduce with a Codex JSONL `command_execution` item where the command is
      `rg` and `aggregated_output` includes a historical capture line containing
      `monsterrodholders-server` and `socket: Operation not permitted`.
- [ ] Confirm the current detector returns a false positive before checking
      `looks_like_ssh_command(command)`.
- [ ] Compare with the legitimate case where command is
      `ssh monsterrodholders-server true` and `aggregated_output` contains only
      `socket: Operation not permitted`.
- [ ] Check streaming and non-streaming Codex paths; both call
      `transcript_has_required_ssh_failure(...)`.

## Implementation Steps

- [ ] Reorder `command_execution_required_ssh_failure()` so generic
      `aggregated_output` scanning cannot fire unless the command is an SSH
      command targeting a required host, or the JSON event is explicitly marked
      as a required-SSH probe.
- [ ] Keep the existing bare-EPERM detection for actual SSH commands where the
      output omits the host name.
- [ ] Add diagnostics that include the command string when failing a required
      SSH guard.
- [ ] Consider ignoring matches from known historical artifact paths
      (`.agent-doc/captures/`, `.agent-doc/logs/`) as a defensive belt after
      command-context gating.

## Regression Coverage

- [ ] Unit test: `command="rg socket .agent-doc/captures"` with output
      containing `monsterrodholders-server` and `socket: Operation not
      permitted` returns `None`.
- [ ] Unit test: `command="ssh monsterrodholders-server true"` with bare
      `socket: Operation not permitted` still returns the failure.
- [ ] Unit test: `command="ssh root@50.28.2.199 true"` with network/auth/DNS
      failure still maps to the configured alias match terms.
- [ ] Integration-style run test: a child agent may inspect old captures without
      aborting the active response cycle.

## Exit Criteria

- Required-SSH capability failures are only raised from live SSH proof/command
  evidence.
- Searching historical captures or logs cannot abort an unrelated `agent-doc`
  child run.
- Legitimate `socket: Operation not permitted` from an actual SSH attempt still
  fails closed before the agent reports success.
