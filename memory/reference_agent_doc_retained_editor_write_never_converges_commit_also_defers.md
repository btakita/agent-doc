---
name: reference_agent_doc_retained_editor_write_never_converges_commit_also_defers
description: "A retained editor-owned write can sit in write_applied indefinitely while write --commit AND commit both defer with \"CRDT relay convergence still pending\" — the binary's own \"it commits itself\" guidance is a no-op, and only an editor-side save/close unblocks it."
metadata: 
  node_type: memory
  type: reference
  originSessionId: 5093a843-114e-42ac-93fd-a1a737a5a099
  modified: 2026-08-02T06:15:49.245Z
---

Observed 2026-08-02 on `tasks/software/lazily.md` (agent-doc 0.35.109, JB plugin
0.2.334, IDEA live with the file open in a tab).

**Shape.** `agent-doc write --commit` applies the response to disk, then fails:

```
serialized_atomic_write: retained editor-owned write ... before retrying live model reconciliation
  visible document write is retained by the lazy delivery projection because the
  editor state projection has not converged; no secondary snapshot/commit or
  forced disk write was attempted
```

`session-check` then reports `INTERRUPTED: cycle ... is still write_applied —
response write landed but no terminal commit followed`, and **stays there
indefinitely** (polled ~9 minutes, no state change).

**What makes this variant distinct from the neighbours:**

- Not a stale plugin — the binary itself prints
  `live jetbrains plugin 0.2.334 registered (expected 0.2.334); this live
  generation supersedes any stale plugin warning captured earlier in the turn`.
- Not a controller wedge — `admin inspect` shows
  `state=ready queue_control=none projection_lag=false
  freshness=controller:fresh,supervisor:fresh`, and `admin detect` reports no
  fleet anomalies. Every diagnostic is green while the cycle cannot close.
- **`agent-doc commit <FILE>` does NOT clear it.** That is the important
  correction to [[reference_agent_doc_retained_pending_write_superseded_hash_agent_doc_commit]]:
  `commit` gets further (`repositioned boundary in snapshot`) but ends at the
  same deferral — `CRDT boundary refresh retained pending exact editor receipt`.
- Bare `write --commit` with no stdin also does not clear it; it correctly
  detects `Response already present in document; skipping apply and retiring
  retained intent`, advances the snapshot, and then hits the same wall.

**Why:** the error text asserts "the same intent commits itself once delivery
converges — this is a deferral, not a lost response" and explicitly forbids
force-disk, `admin recycle`, and `admin reload-lib`. Treat that promise as
unverified. When the editor projection is the thing that is wedged, nothing on
the agent side converges it, so waiting is unbounded.

**CORRECTION 2026-08-08** (same file `tasks/software/lazily.md`, JB plugin
0.2.350). Same error text, and `respond --stream` reported
`failed to apply tracked-work mutations` — but **every mutation was already on
disk**: response section, all five `--backlog-add` items, the `--done` reap, and
the drained queue head. Only the terminal commit was missing. Bare
`agent-doc write --commit <FILE> </dev/null` then **DID** clear it in one shot:
`Response already present in document; skipping apply and retiring retained
intent` → commit_authority → `session-check ok — cycle is committed`. So the
"bare `write --commit` hits the same wall" bullet above is version-specific, not
a law; the newer binary/plugin pair retires the retained intent.

**How to apply:** poll `session-check` two or three times; if it is still
`write_applied`, **read the document first** — the mutations are usually already
applied, so never re-run `respond` (it would duplicate). Then run bare
`write --commit` with no stdin, which is the same recovery as
[[reference_agent_doc_write_applied_no_terminal_patchback_guard_misfire]]. Only
if that still defers is this the operator-gated variant: the resolutions are
editor-side (exact save-flush, a newer editor edit, closing the file). Do NOT
force disk in either case: the editor holds authority and forcing would drop its
copy. The response is already durable on disk, so nothing is lost by stopping.

Related: [[reference_agent_doc_stale_plugin_ack_wedge_reload_lib_unwedge]],
[[reference_agent_doc_write_applied_no_terminal_patchback_guard_misfire]],
[[reference_agent_doc_deferred_write_may_have_already_applied_mutations]].

**UPDATE 2026-08-27** (`tasks/software/existence-lang.md`, twice in one
session, `respond --stream` with `--done` + `--backlog-add`): same error text
(`half-applied ... [recovery=await_editor_replica_no_disk_write]`), every
mutation already on disk, `session-check` = `write_applied`. The error's own
first remedy, `agent-doc commit <FILE>`, cleared it in one shot both times
(`session-check ok — committed`). So on a current binary: check disk, run
`agent-doc commit`, then `session-check`; no wait, no resend.
