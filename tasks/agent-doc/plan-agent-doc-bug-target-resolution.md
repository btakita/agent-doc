# Plan: #agent-doc-bug target resolution from nested session repos

## Problem

When a session document lives under a nested repository such as
`src/session-share`, the `#agent-doc-bug` preset can refer to the parent
project target:

```text
agent-loop/tasks/agent-doc/agent-doc-bugs2.md
```

The closeout can still write or prompt the agent to write under the current
nested workspace, for example:

```text
src/session-share/tasks/agent-doc/agent-doc-bugs2.md
```

That silently records the bug in the wrong mirrored document. This is distinct
from the completed `#pendrev` ordering bug and from the older completed
`#crossdocpend` current-document fallback bug: the failure here is target path
authority across nested repositories with similar task trees.

## Fix

1. Normalize `#agent-doc-bug` backlog targets against the owning project root
   declared by the session, not just the process current directory.
2. If a target path includes a known project directory name such as
   `agent-loop/`, resolve it to that project root before considering local
   relative candidates.
3. Fail closed when the resolved target is outside the intended project or when
   multiple matching task trees exist.
4. Surface the resolved target path in the preflight/closeout diagnostics so
   the agent can verify the bug will be written to the intended backlog.

## Tests

1. Session file in `src/session-share/tasks/root.md` with target
   `agent-loop/tasks/agent-doc/agent-doc-bugs2.md` resolves to
   `/home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md`.
2. Same nested session with a local mirrored
   `src/session-share/tasks/agent-doc/agent-doc-bugs2.md` does not choose the
   local mirror.
3. Ambiguous or missing project-root target fails before writing any backlog
   item.
