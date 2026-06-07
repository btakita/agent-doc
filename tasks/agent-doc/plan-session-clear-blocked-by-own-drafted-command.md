# Plan: `session clear` blocked by its own drafted clear command

`#clear-blocked-by-own-drafted-clear-command`

## Symptom (live, 2026-06-06)

After stopping the Codex session, the supervisor is still running and the
operator cannot clear it:

```
❯ agent-doc session clear tasks/monsterrodholders.md
Error: session_clear refused for .../monsterrodholders.md because pane %29
contains protected prompt input (reason=drafted prompt input,
source=authoritative_actor, current_command=agent-doc,
tail="❯ agent-doc session clear tasks/monsterrodholders.md")
```

User: *"I cannot clear the monsterrodholders.md session after stopping the Codex
session. The supervisor is still running. The supervisor needs to be more
'intelligent'."*

## Root cause (hypothesis — needs confirmation)

The protected-prompt guard that prevents `session clear` from discarding a real
drafted user prompt is matching the operator's **own clear command text** echoed
in the pane composer (`tail="❯ agent-doc session clear ..."`). The drafted text
that blocks the clear *is the clear command itself*, so the guard is
self-defeating: the only way to satisfy it is to manually erase the line the
operator just typed, or fall back to `interrupt-clear`.

## Immediate operator recovery (already available)

The error message names the escape hatch:

```
agent-doc session interrupt-clear tasks/monsterrodholders.md
```

`interrupt-clear` intentionally interrupts the pane and clears context, bypassing
the protected-prompt guard. So the operator is not truly stuck — but the plain
`clear` UX is wrong.

## Fix direction

1. The drafted-prompt protection must **exclude agent-doc control commands** from
   the "protected prompt input" set. A composer line that is itself an
   `agent-doc session clear|interrupt-clear|stop|...` invocation is an operator
   control action, not unsaved creative prompt content worth protecting. Detect
   that the drafted `tail` is an agent-doc control command and let `clear`
   proceed (it is about to discard that line anyway).
2. Alternatively / additionally: when `current_command=agent-doc` and the drafted
   tail matches the clear command being run, treat it as a confirmed clear intent
   rather than a conflict.
3. Locate the guard: search `session_actor_cmd.rs` for the `drafted prompt input`
   / protected-prompt refusal and `reason=drafted prompt input`. Keep aligned
   with `#codex-interrupt-clear-ctrl-g-opens-editor` and the
   `session_clear_live_busy_guard_blocked` family (`#clear-stops-running-process`,
   `#autoloop-command-preemption`).
4. Deterministic test: drafted tail = the clear command → clear proceeds; drafted
   tail = genuine prompt content → clear still refuses (regression guard).

## Related

`#clear-stops-running-process`, `#autoloop-command-preemption`,
`#codex-interrupt-clear-ctrl-g-opens-editor`, the stale-busy supervisor cluster
(`#stale-busy-after-auto-inject-no-clear`).
