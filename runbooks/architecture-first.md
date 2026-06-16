# Architecture-First Implementation

Canonical agent-doc instructions live at
[`../.claude/skills/agent-doc/runbooks/architecture-first.md`](../.claude/skills/agent-doc/runbooks/architecture-first.md).

Use that runbook for agent-doc implementation turns that mention architecture,
sprawl, invariants, refactors, state machines, or bugs crossing route, repair,
session-check, write, controller state, sidecars, or editor IPC.

Minimum contract before editing:

```md
Invariant:
Policy owner:
Transition table:
Evidence inputs:
Allowed edit surfaces:
Verification:
Out of scope:
```

If the invariant or policy owner cannot be named, stop with a plan instead of
adding another local fix.
