---
name: reference_agent_doc_retained_pending_write_superseded_hash_agent_doc_commit
description: "Retained pending_write wedge whose target hash was superseded (payload_absent_from_converged_content) — `agent-doc commit <FILE>` clears it; session-check/reload-lib/write --commit all refuse"
metadata:
  node_type: memory
  type: reference
---

Seen 2026-07-26 on `tasks/agent-doc/agent-doc-bugs2.md` (agent-doc 0.35.28).
`preflight` and `session-check` both fail closed with:

```
[session-check] INTERRUPTED: binary-owned response delivery `<id>-<n>-<hash>` is
retained ... (reason=crdt_delivery_ack_pending, source=pending_write,
target_hash=<hash>); ... retry only `agent-doc session-check <FILE>`.
```

It never converges. The distinguishing evidence is in `.agent-doc/logs/ops.log`:
`retained_write_blocks_new_cycle ... cause=payload_absent_from_converged_content`,
and `crdt_current_text ... delivery_converged=true` for a **different** hash than
the retained `target_hash`. Sequence that produces it: `pending_write` applies
target A and is ACKed, then a follow-on `pending_add_sync` (the queue/backlog
mirror) applies A+queue-lines as target B seconds later, the commit barrier
blocks (`pre_commit_guard outcome=blocked reason=replica_delivery_pending`), and
the closeout aborts. The retained intent now waits for an exact hash the document
has already superseded — a permanent gate, not a transport problem.

**Not a delivery/ACK wedge, so the usual ladder does nothing.** `admin inspect`
shows `state=ready freshness=controller:fresh,supervisor:fresh`, `admin detect`
finds no anomalies, `doctor` reports `editor_convergence outcome=ok`, and
`admin reload-lib` delivers to 1/1 endpoints and changes nothing. `doctor` says
`closeout_commit outcome=recoverable repair=agent-doc write --commit <FILE>`, but
bare `write --commit` returns `Error: empty response — nothing to write` (the
response body is already flushed to the working tree, so there is nothing on
stdin and it does not take the already-present-response path). `agent-doc repair`
just re-prints the same INTERRUPTED error. `autofix` only plans that same
`write --commit`.

**Fix: `agent-doc commit <FILE>`** (exit 0, silent). It commits the
already-flushed response and clears the retained gate; the next `session-check`
returns `ok — cycle ... is committed (capture_committed)`. Do NOT force-disk —
the working tree already holds the right content and a force-disk during an
editor-reconnect window duplicates the response
([[reference_agent_doc_stale_plugin_ack_wedge_reload_lib_unwedge]]).

Follow-on: after `commit`, `preflight` may report `snapshot differs from HEAD
without an open or recoverable agent-doc cycle` and print `agent-doc commit` +
re-run preflight; that is the documented path, take it. It may also defer with
`Lazily current authority remained delivery_pending`/`authority_unavailable` for
a minute or two while the controller recycles onto a fresh binary — retry rather
than escalate.

**Do not run `preflight` more than once per turn while recovering.** Each extra
call used to walk the queue forward one head (`#adpreflightreentryskip`); fixed
in agent-doc `b15557527`, but the habit is what triggered it.
[[reference_agent_doc_recovery_index]]
