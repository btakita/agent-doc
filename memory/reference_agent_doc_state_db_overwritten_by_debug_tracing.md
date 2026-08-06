---
name: reference_agent_doc_state_db_overwritten_by_debug_tracing
description: "agent-doc's state.db gets overwritten by its own debug tracing stream — file starts with \"agent-doc tracing initialized\", not the SQLite magic; recovery kills OTHER sessions"
metadata: 
  node_type: memory
  type: reference
  originSessionId: a5a9e782-82c8-4457-962f-9adae81c9c8a
  modified: 2026-07-26T20:03:27.336Z
---

**Seen 2026-07-26 19:46, and it had happened before** (a 73.1M
`.agent-doc/state.db.first-corruption.bak` was already sitting beside it).

Symptom: every `agent-doc` command dies with

```
Error: authoritative state db .../.agent-doc/state.db is unavailable; refusing automatic replacement
  0: state db is corrupt  1: file is not a database  2: Error code 26
```

**The diagnosis is one command** — check the file header:

```bash
head -c 16 .agent-doc/state.db | xxd
```

A healthy db starts with `SQLite format 3\0`. The corrupt one started with an
ISO timestamp, and the first line was:

```
2026-07-26T19:46:39.025524Z DEBUG ThreadId(01) agent_doc: agent-doc tracing initialized (filter: debug)
```

959 lines of tracing, then the original binary db content further in. So a
**debug-log writer was pointed at the state.db path** and clobbered its head.
That is an agent-doc bug, not disk damage — and `RUST_LOG=debug` / a debug-built
`agent-doc controller serve` in the process list is the thing to correlate with.

**Do not run the printed recovery unposted.** Step 1 is
`pkill -f 'agent-doc (controller|start) '`, and on this workspace that matched
**5 live processes across several sessions** (other panes' `start --route-owned`
documents plus a controller). Killing them takes down other people's live
sessions. Confirm with the operator, or scope the kill to the pid that owns this
project root.

The rest of the recovery is safe and non-destructive **because the corrupt file
is preserved, not deleted**:

```bash
mv .agent-doc/state.db .agent-doc/state.db.corrupt-$(date +%s)
agent-doc admin detect   # recreates a clean db
agent-doc fix            # restore panes
```

`state.db` holds cycle/closeout state only — documents live in git and the
session registry re-derives from live panes, so rebuilding loses no work.

Preceding signal worth noting: the same session had already hit
`failed to persist controller model pressure in state.db: database is locked`
during a `session-check`, minutes before the file went non-database.
