---
name: reference_agent_doc_write_applied_no_terminal_patchback_guard_misfire
description: "Cycle stuck at `write_applied` — the response IS on disk but `commit`/`write --commit` refuse with \"likely direct response patchback without agent-doc cycle\"; the guard misfires on the binary's OWN applied write."
metadata: 
  node_type: memory
  type: reference
  originSessionId: 9dd22c19-7622-43b4-bf89-4cd9f14119a3
  modified: 2026-08-01T22:01:27.204Z
---

Observed 2026-07-29 on `tasks/professional/equityfundingsource.md` (binary 0.35.96→0.35.97,
live JetBrains plugin 0.2.322 vs the 0.2.323 the binary ships).

**Symptom chain**, in order:

1. `respond --stream` → `serialized_atomic_write: retained editor-owned write ... timed out
   after 5.0s waiting for project controller response`.
2. `session-check` → retained delivery, `reason=merge_unsaved_editor_cut_with_deferred_target`,
   `source=editor_reconnect`, and an explicit "do NOT run commit / force disk / resubmit;
   retry only session-check".
3. `admin reload-lib` reveals the real cause: `IPC build mismatch: listener=0.35.97,
   client=0.35.96` — a concurrent session installed a newer binary mid-turn, so **your CLI is
   the stale peer**. Reload delivers to only 1/2 endpoints (1 unavailable = the stale plugin).
4. After the client catches up, the state advances to **`write_applied`** — the response body IS
   now in the document on disk and the queue head/`--done` mutations DID apply.
5. But `agent-doc commit` and `agent-doc write --commit` both refuse:
   `refusing to treat <FILE> as already committed: found likely direct response patchback
   without agent-doc cycle: ### Re: <topic>`.

That last guard is **wrong here**: the response was applied by the binary itself
(`patch_id=response-cell:cycle-<id>:<hash>` is in the cycle record). It only *looks* like a
manual patchback. `admin detect` reports no anomalies and `admin recycle` just defers to the
next idle boundary, so nothing self-heals.

**Recovery — try this first (worked 2026-07-30, binary 0.35.104, on
`tasks/software/tsift.md`).** `agent-doc commit` still refuses with the misfiring patchback
guard, but bare **`agent-doc write --commit <FILE>` with no stdin body does NOT** — it reaps the
pending `--done`, commits, and only then fails `session-check` with "document still contains
completed tracked item(s) after closeout ... re-run preflight/repair so the reap is persisted".
Doing exactly that (`agent-doc preflight <FILE>`) persists it: `committed: true`,
`no_changes: true`, backlog empty, `queue_active: false`. Three commands, no hand-commit:

```
agent-doc write --commit <FILE>   # no stdin — the body is already on disk
agent-doc preflight <FILE>        # persists the reap through snapshot + commit
```

A trailing `session-check` may still report `canonical editor authority and disk projection
diverge` — it says so itself: the controller owns the next attempt and wakes on that state edge.
Do not resubmit finalize or ask the operator to save.

**Reconfirmed twice on 2026-08-01** (binary 0.35.109, `tasks/software/tsift.md`), and the
loop is reproducible: `respond --stream` → `serialized_atomic_write: ... remains retained after
the foreground editor ACK deadline` → poll `session-check` until it flips from
`crdt_delivery_ack_pending` to **`write_applied`** (one attempt, ~15s) → bare `write --commit`
→ `preflight`. Two additions to the above:

- **`write --commit` can exit non-zero and still have committed.** It printed
  `Error: refusing to close <FILE> as already committed: the staged snapshot matches HEAD, but
  the editor-authoritative document has uncommitted queue, backlog, status, or other typed-
  component drift without an exact binary-owned retained-target proof` — yet `git log -1 --
  <doc>` showed the closeout commit, the `### Re:` body, the queue strike, and the
  `--done`/`--backlog-gate` mutation all landed. **Verify from git, not from the exit status.**
  The residual working-tree drift is the post-commit queue/backlog bookkeeping the binary says
  it is "leaving ... uncommitted".
- The follow-up `preflight` may itself fail with `Lazily state actor rejected closeout fact
  append: a retained document-write effect still owns the prior captured response`. That is also
  not a failure to chase — `session-check` right after returned the terminal
  `ui_outcome=deferred_for_supervisor_drain` / `next_action=yield_to_supervisor_clear_and_continue`,
  which is the yield signal, not a stall.

Precondition that predicted it both times: preflight's `stale_install` warning named
`f0c3efd0f fix: reactively reconcile retained closeout projections` as a landed-but-not-installed
commit — i.e. the fix for this exact wedge was sitting uninstalled while the wedge fired.

**Fallback recovery** if `write --commit` also refuses (non-destructive; do NOT `--force-disk`,
do NOT kill `controller serve` — that takes out every other pane's live session):

1. Confirm the content is actually complete on disk: response section present, queue drained,
   `--done` id resolved and archived into the `.done.md`, status correct.
2. Commit the session document + its `.done.md` by hand with
   `git commit --only -- <doc> <doc>.done.md` (see
   [[reference_shared_superproject_index_contamination_commit_only]] — plain `git add` will
   swallow another pane's staged files).
3. Bump the affected submodule gitlink in a separate `--only` commit.

`session-check` will keep reporting `write_applied ... no terminal commit followed` afterwards —
that is agent-doc's own state record, not a content problem. Do not chase it with more closeout
payloads.

**Precondition worth acting on earlier:** preflight warned about both `stale_install` (binary
missing `805b71d9c fix: make closeout and restart continuations reactive`) and `stale_plugin`
(0.2.322 < 0.2.323) at the *start* of the turn. Editor delivery ACKs cannot converge with a
stale plugin, so this class of wedge is predictable from the preflight warnings — the plugin
update + IDE reload is an operator action. See
[[reference_agent_doc_stale_plugin_ack_wedge_reload_lib_unwedge]] and
[[reference_agent_doc_stale_plugin_heartbeat_forcedisk_recovery]].
