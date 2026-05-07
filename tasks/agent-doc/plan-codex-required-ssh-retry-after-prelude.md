# Plan — retry required-SSH resume drift even after Codex emits prelude text

**Pending:** `#sshprelude`
**Status:** Draft

## Problem

May 6, 2026 exposed a narrower required-SSH bug in the Codex streaming path.

Observed behavior:

- direct SSH from the host shell worked:
  - `ssh monsterrodholders-server 'hostname && whoami && pwd'`
- a fresh `codex exec --json -s danger-full-access` session also worked with the
  same SSH alias
- but the resumed `agent-doc` turn in
  `src/boost-client/tasks/monsterrodholders.md` kept claiming local SSH config
  failure (`Bad owner or permissions on /etc/ssh/ssh_config.d/20-systemd-ssh-proxy.conf`)
  even after the user had already proved `ssh monsterrodholders-server true`
  from the same machine

The archived turn shows the resumed Codex thread emitted ordinary assistant
prelude text first ("I’m retrying..."), then later surfaced the required-SSH
failure. That sequence matters because the current streaming retry logic only
fresh-restarts a resumed Codex session while `yielded_agent_content == false`.

So the effective failure mode is:

1. resumed Codex thread carries stale SSH capability or stale local-client view
2. it emits harmless text before attempting the SSH command
3. the later SSH failure is detected
4. `agent-doc` refuses the fresh-session recovery path because content already
   streamed
5. the user sees repeated bad advice from a poisoned resumed session even though
   a fresh Codex session can SSH successfully

## Desired Behavior

1. A resumed Codex session that loses required SSH capability should still get
   one fresh-session retry when the failing event proves required SSH drift,
   even if the thread already emitted non-final assistant text.
2. The stale prelude text from the poisoned resumed session must not leak into
   the committed response if `agent-doc` decides to discard that thread and
   relaunch fresh.
3. Required-SSH recovery should stay fail-closed: if the fresh session still
   cannot prove the SSH capability, surface the blocker clearly.
4. The existing localhost/CDP capability-drift handling must stay separate.

## Likely Root Cause

The blocking Codex path already retries on required-SSH drift after `send_once`
returns, regardless of whether the response text buffer was populated. The
streaming path is stricter.

In `src/agent-doc/src/agent/codex.rs`, `CodexStreamIterator` currently gates the
fresh retry on `!self.yielded_agent_content` in both places where required-SSH
drift is handled:

- while processing JSONL lines
- after process exit when stderr carries the failure

That means any assistant text chunk emitted before the SSH command permanently
disables fresh-session recovery for the rest of that resumed stream, even when
the later failure clearly proves required SSH drift.

## Implementation Steps

- [ ] Reproduce the exact sequence with a focused streaming regression:
  - resumed Codex stream
  - `required_ssh_targets: [monsterrodholders-server]`
  - first an `agent_message` prelude chunk
  - then a `command_execution` SSH failure item
  - verify the current iterator does not restart fresh
- [ ] Decide the discard boundary for stale resumed output:
  - safest default is to buffer streamed assistant text until the session has
    crossed a "capability-safe" point, or
  - explicitly clear any buffered prelude text when required-SSH drift forces a
    fresh restart
- [ ] Relax the retry gate for required-SSH drift in the streaming iterator so
  it does not depend on `yielded_agent_content == false`
- [ ] Preserve the one-retry limit:
  - resumed thread may restart fresh once
  - fresh thread failure still returns a fail-closed required-SSH error
- [ ] Verify the same rule for stderr-only drift detection, not only inline
  JSONL events
- [ ] Keep localhost/CDP EPERM on its separate capability-drift branch
- [ ] Add a regression proving the user-facing contract:
  - resumed stream emits prelude text
  - required SSH later fails
  - `agent-doc` retries fresh
  - the final visible output comes only from the fresh successful thread

## Files Likely To Change

- `src/agent-doc/src/agent/codex.rs`
- `src/agent-doc/specs/05-agent-backend.md`
- `src/agent-doc/README.md`
- `src/agent-doc/VERSIONS.md`

## Exit Criteria

- A resumed Codex stream cannot get stuck on stale required-SSH state merely
  because it emitted a short assistant prelude before the failing SSH command.
- When fresh Codex can SSH successfully, `agent-doc` recovers to that fresh
  thread instead of repeating stale resumed-session diagnostics.
- Any discarded prelude text from the stale resumed thread does not pollute the
  final committed response.
- Regression coverage captures the exact "assistant prelude, then SSH failure"
  sequence from this report.
