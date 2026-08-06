---
name: reference_agent_doc_state_db_header_clobbered_by_tracing_on_controller_recycle
description: "state.db's first 104 bytes get overwritten by an `agent-doc tracing initialized` line during a stale-binary controller replacement — every document in the project loses closeout state in the same second"
metadata:
  node_type: memory
  type: reference
---

Symptom: every agent-doc command on the project fails with

```
Error: authoritative state db .../.agent-doc/state.db is unavailable; refusing automatic replacement
  0: state db is corrupt: .../.agent-doc/state.db
  2: Error code 26: file is not a database
```

The file is NOT garbage. Only bytes 0-103 are clobbered — the 100-byte SQLite
header plus 4 bytes of page 1 — by exactly one line of tracing output:

```
2026-07-26T19:46:39.025524Z DEBUG ThreadId(01) agent_doc: agent-doc tracing initialized (filter: debug)
```

Everything after byte 104 is intact SQLite (`grep -abo "SQLite format 3"` finds
nothing, but the `state_schema_migrations` index page is readable at ~byte 300).
Confirm with `head -c 20 .agent-doc/state.db | cat -v`.

**Trigger (2026-07-26 19:46:39Z, agent-loop superproject):** a stale-binary
controller replacement right after `make install` + `agent-doc admin recycle`.
The ops.log second-by-second:

- `controller_shutdown_accepted pid=... request_reason=stale_controller_replacement controller_binary_stale=true`
- `controller_serve_inherited_fds_closed count=1` — the replacement process found
  **one inherited fd**
- state.db mtime == this second, header replaced by the line above
- `captured_finalize_resume_key_error` for **every** open document in the same
  second (`lazily.md`, `job-offers.md`, `agent-doc-bugs2.md`)

`AGENT_DOC_LOG=debug` is set in this environment, so `init_tracing()` (main.rs)
emits that line on every process start. It runs in `main()` **before** the
`controller serve` handler closes inherited fds, which is the ordering worth
suspecting. Not proven — do not state the mechanism as fact without a repro.

**Blast radius is the whole project**, not one document: `.agent-doc/state.db` is
superproject-scoped, so a concurrent session that looks like an unrelated
"commit stalled on CRDT relay convergence" churn loop is usually this. Check
whether its last ops.log event is `captured_finalize_resume_key_error` at the
same second before debugging it as a separate bug.

Recovery (operator-run — step 1 kills the route-owned supervisor hosting the
Claude session, so an agent cannot run it without ending itself):

```bash
pkill -f 'agent-doc (controller|start) '
mv .agent-doc/state.db .agent-doc/state.db.corrupt-$(date +%s)
agent-doc admin detect   # recreates a clean db
agent-doc fix            # restore panes
```

state.db holds only rebuildable cycle/closeout state — documents are in git and
the session registry re-derives from live panes. **Preserve the corrupt copy**;
it is the only evidence. Do NOT splice a header from `state.db.first-corruption.bak`:
the header carries the page count and change counter, so a wrong value makes
SQLite mis-read the file successfully instead of failing loudly.

Recurring: `.agent-doc/` already held `state.db.corrupt-1784785883*` and a
73MB `state.db.first-corruption.bak` before this event.
[[reference_agent_doc_recovery_index]]
