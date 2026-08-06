---
name: reference_agent_doc_compact_committed_but_visible_rewrite_deferred
description: "compact commits the compacted blob to git but leaves the FULL uncompacted doc in worktree+editor (post_commit_reposition_deferred, recovery=editor_reconnect that never comes); reload-lib does NOT flush it — repair with `agent-doc patch <FILE> exchange` fed HEAD's exchange body"
metadata: 
  node_type: memory
  type: reference
  originSessionId: c54d12a1-95e4-49a0-b66d-0178f0a3a3d8
  modified: 2026-07-25T18:07:23.946Z
---

Symptom (2026-07-25, agent-doc-bugs2.md): "Compact Exchange committed the compacted
exchange but the uncompacted exchange came back as uncommitted." It never came back —
it was **never removed**. `compact` committed the compacted blob to git, and the
visible-document rewrite was deferred and never applied, so worktree + editor + CRDT
all stayed on the pre-compact text while HEAD alone was compacted.

**Diagnose (this exact signature, cheap):**
```bash
stat -c '%y %s' <FILE>            # mtime PREDATES the compact commit => disk never rewritten
sha256sum <FILE>; git show HEAD:<FILE> | sha256sum
grep -a post_commit_reposition .agent-doc/logs/ops.log | tail -2
```
The clincher: `post_commit_reposition_deferred ... transport=lazily_deferred
target_hash=<H> recovery=editor_reconnect`, where `<H>` is **exactly** the sha256 of
`git show HEAD:<FILE>`. `agent-doc read` (live authority) returns the stale text, and
`doctor` reports `editor_convergence ok` — it is converged, on the WRONG side.

**Do NOT** run `write --commit` here even though `doctor`/`session-check` recommend it
(`closeout_commit recoverable`, "direct response patchback ### Re: ..."). Those visible
`### Re:` sections are the pre-compact exchanges; absorbing them commits the doc
**back to uncompacted** and undoes the compaction.

**`admin reload-lib` does not fix it.** It delivered to 1/1 endpoints and produced a
real `crdt_relay_hub_evicted` -> `crdt_replica_deregister` -> `crdt_replica_register`
reconnect, and the retained intent still did not replay. This is the live proof of
backlog item **#0dsr** ("a retained intent waits for a reconnect that may never come");
`deferred_document_write_reconnect_content` never fired and emits no ops.log event.

**Repair that worked** — replace the exchange component with HEAD's own compacted body,
through the normal write path (CRDT + editor + disk, no `--force-disk`):
```bash
git show HEAD:<FILE> > /tmp/head.md
sed -n '/<!-- agent:exchange/,/<!-- \/agent:exchange/p' /tmp/head.md \
  | sed '1d;$d' > /tmp/exchange.txt        # body only, keeps the single agent:boundary line
agent-doc patch <FILE> exchange < /tmp/exchange.txt
```
Safe because every other component was already byte-identical to HEAD; only the
exchange diverged. Verify: `sha256sum <FILE>` == HEAD hash, `agent-doc read | wc -l`
matches, `git status --porcelain <FILE>` empty, `doctor` flips `closeout_commit` to ok.

**Before discarding the worktree text, prove nothing is lost**: every `### Re:` heading
in the worktree that is absent from HEAD must appear in the archive named by HEAD's
`*Compacted. Content archived to ...*` line. In this incident the only worktree-unique
line was the superseded `<!-- agent:boundary:... -->` marker.

Related: [[feedback_post_commit_worktree_drift_checkout_head_recovery]],
[[feedback_compaction_stale_snapshot_reset_not_commit]],
[[feedback_boundary_accum_worktree_only_ipc_repair_checkout_head]] (that one is
worktree-only corruption with no live supervisor -> `git checkout HEAD --`; this one has
a LIVE editor holding the stale text, so checkout alone would be clobbered back),
[[reference_agent_doc_recovery_index]].
