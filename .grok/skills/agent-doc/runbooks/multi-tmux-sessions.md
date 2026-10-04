# Multi-session tmux projects

Use this runbook when one editor should manage agent-doc panes across several
tmux sessions, for example one session per topic or per project (GH #17).

## Configure the allowed sessions

List every session agent-doc may target in `.agent-doc/config.toml`:

```toml
tmux_session = "main"                  # default pin for unbound documents
tmux_sessions = ["main", "research"]   # allowed sessions; empty = single-session
```

Leaving `tmux_sessions` out (or empty) keeps the single-session model exactly
as before. When it is set, the `tmux_session` pin must be one of the listed
sessions; `agent-doc session set <NAME>` refuses a name outside the list and
only moves the default pin. It never moves windows out of, or closes, another
allowed session.

## Bind a document to a session

Add `tmux_session: <name>` to a document's frontmatter to give it a topic
session:

```yaml
---
agent_doc_session: 7c1e...
tmux_session: research
---
```

The binding only takes effect in multi-session projects. In single-session
projects the field stays deprecated and routing ignores it.

## How a document picks its session

Route, start, sync, and layout resolve the target in this order:

1. An explicit window (`sync --window`, `terminal --session`).
2. The session of the document's existing live pane.
3. The document's `tmux_session` frontmatter binding.
4. The caller's current session, when it has an `agent-doc` window.
5. The project `tmux_session` pin, when that session is alive.
6. The ambient fallback (the current tmux session, then the harness default).

Every result must be in `tmux_sessions`. Candidates 4 and 6 come from the
caller's terminal, so a disallowed one is skipped and the next candidate is
used. A disallowed explicit window, existing pane, binding, or pin fails closed
with an error naming the allowed list. `--force` does not widen the list. Edit
`tmux_sessions` or the document binding instead.

`start` keeps a pane that is already in an allowed session where it is. It
relocates a pane only to the document's binding, or to the allowed pin when the
pane sits in a disallowed session. A sync arranges one session at a time: layout
columns whose documents are bound to a different session are dropped from that
sync (`cross_session_layout_dropped` in the sync log) and keep their panes in
their own session.

## Checks

- `agent-doc session` prints the pin and an `allowed:` line.
- A refusal reads `tmux session '<name>' (from <source>) is not in the allowed
  tmux_sessions list [...]`. The source names which input chose it.
- `agent-doc resync` treats an unbound pane in any allowed session as correctly
  placed, and a bound pane as belonging to its binding.
