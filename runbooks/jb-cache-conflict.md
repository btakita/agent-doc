# JetBrains File Cache Conflict

After a controller recycle or binary upgrade, an open JetBrains tab may outlive its server-side relay membership. Re-registration always opens the controller bootstrap and projects that retained canonical revision into the editor. The pre-existing IntelliJ `Document` is not a recovery baseline: it may predate queue additions or acknowledged edits from another endpoint. A `reload_lib` broadcast can rebuild the downstream consumer, but it cannot publish the whole editor buffer upstream.

`--force-disk` remains an explicit operator choice, but it does not outrank an open editor. Before writing disk, the binary stores the full pre-write editor cut and disk target in an independent Lazily external-disk slot. It never component-merges that candidate into the editor. The ordinary pending response lineage remains separate, so a file-cache decision cannot replace or clear a retained agent response. The recovery source is Lazily state, never Git HEAD.

Captured-response recovery is controller-authoritative. The final capture and exact canonical target live in the controller-owned keyed Lazily graph. If the editor is unavailable or has not acknowledged that revision, the operation remains retained and returns promptly; snapshot, commit, and disk projection do not run ahead of the visible receipt.

When IntelliJ has a session document open, an older or degraded write path can surface a **File Cache Conflict** dialog because the editor cache disagrees with a competing disk or legacy IPC mutation. The current attached-document path prevents that race by treating CRDT delivery as the only visible-document mutation plane.

Durable CRDT deltas are lineage-scoped. Stale or obsolete lineages are
quarantined. They wake the controller's retained canonical projection, never a
full-state request, editor adoption, or canonical reconstruction.

## External Disk Pending Lifecycle

Any filesystem change observed while at least one editor buffer is open is a pending disk candidate. The watcher records it in Lazily but does not replace controller canonical state. The authority order is controller canonical, then disk after detach and proof, with Git only as historical evidence.

- A controller revision is delivered downstream and acknowledged by exact content.
- A subsequent operator `DocumentEvent` publishes only its causal incremental delta.
- A stale opening buffer, failed ACK, or reconnect never publishes whole editor text.
- Disk candidates remain pending until detached authority is proven or an explicit operator action resolves the conflict.
- Closing one of several editors does not demote controller authority.

## Quiescent CRDT Delivery

For a document with an attached CRDT replica, every ordinary binary-owned write follows this contract. The explicit operator-authorized `--force-disk` recovery remains the documented escape hatch and keeps actor serialization while intentionally bypassing CRDT delivery.

CRDT bootstrap/delta payloads use the compact UTF-8-safe `ADCR1:` envelope over the existing plugin string seam. The binary still accepts legacy JSON, so `agent-doc admin reload-lib` can upgrade a live plugin and finish an update that was retained by the previous binary. The compact envelope prevents per-character CRDT operations and tombstones from inflating an ordinary session document into a controller payload large enough to starve its delivery ACK.

1. Observe controller canonical state and retain the desired target in the per-document Lazily key.
2. Derive delivery and settlement from canonical revision, membership, visible receipt, and disk observation.
3. Let one keyed editor-delivery effect project the exact canonical revision.
4. On exact receipt, let the settlement effect persist and resume the captured closeout.
5. On missing receipt, return the typed retained/pending outcome immediately. Do not poll for eight seconds, request ACK replay or refresh, adopt editor text, checkpoint a CRDT sidecar, snapshot, or commit.

When `session-check` reports a retained pending document write, reuse that capture. Registration or a normal controller delivery event wakes the same Lazily projection; no recovery command is manufactured.

Response-only finalize uses the same derived settlement. A durable `ResponseCellAdded` fact is an idempotence receipt, not permission to commit ahead of the visible editor. With no live member or receipt, the full target remains retained and the next controller bootstrap projects it downstream.

The durable visible-write receipt carries the complete editor-visible content. Hashes are validation and lookup fields only. A legacy hash-only receipt cannot authorize current-buffer publication or whole-document adoption; the controller reprojects its retained canonical revision instead.

Delivery proof is not a disk-projection lock. If the canonical document advances
after the proof but before disk projection, the binary retains the original
projection base and complete target, rebases that same intent over the new editor
cut, and repeats the CRDT delivery/ACK barrier. It must not ask the agent to
recapture the response, repeat `finalize` or `write --commit`, or use
`--force-disk`. Response cells and queued follow-up mutations remain part of one
semantic intent and therefore apply at most once. After the bounded foreground
rebases, `session-check` reports the retained binary-owned operation while the
supervisor continues it.

## Automatic Legacy-Replay Convergence

The agent should never need to repair a document. On ordinary preflight, before prompt parsing or diff generation, the binary checks one deliberately narrow legacy corruption signature: the complete agent-doc projection is present byte-for-byte twice (or a power-of-two number of times). After typing settles, the binary coalesces those identical copies through the same CRDT replacement and visible-replica acknowledgement path, then continues preflight from the converged text. It logs `preflight_exact_document_replay action=coalesce|converged transport=crdt` and emits a small `exact_document_replay_coalesced` warning instead of expanding the replay into a giant diff or bogus orchestration request.

The detector requires a complete session frontmatter and component structure and exact byte identity. Any non-identical operator edit, incomplete suffix, or ordinary repeated prose is ineligible and remains untouched. Coverage includes the pure replay policy, attached preflight integration, and SimWorld scenario `preflight_boundary_coalesces_legacy_whole_document_replay_via_crdt`.

The dialog contract below remains the recovery path for older plugins, already-open legacy conflicts, or writes that began before this contract was installed.

## Dialog Contract

- **Accept / Load FS changes** — IntelliJ resolves its cache state. The exact accepted buffer reboots the editor replica and clears the pending disk candidate only after CRDT propagation. A response payload previously blocked by conflict detection is not replayed; its independent Lazily response lineage resumes normally.
- **Cancel / Keep memory changes** — IntelliJ preserves the visible editor buffer. The plugin must not write over that memory state. The next editor mutation or save propagates the editor cut and clears the pending disk candidate without component-merging the disk version. Historically the working tree could be left with a partial write while the cycle stayed at `WriteApplied` — see "Recovery" below.

## Recovery

`agent-doc preflight <FILE>` (and therefore the next harness-driven cycle) **auto-recovers the cancel branch** when the binary-owned write path had already applied the response. The detection signature is:

- cycle phase is `WriteApplied` or `Committed`,
- `snapshot` differs from `HEAD`,
- the working tree matches the snapshot modulo transient `(HEAD)` / boundary markers (no live exchange edits beyond the response).

When that shape is detected, preflight runs the equivalent of `agent-doc write --commit` automatically and records `jb_cache_conflict_cancel_auto_recovery_attempt` / `_succeeded` / `_failed` in `.agent-doc/logs/ops.log`. `agent-doc session-check` returns OK for the same shape so it does not misclassify the wedge as a likely direct response patchback.

### When auto-recovery declines

Auto-recovery fails closed (and the legacy drift bail message is surfaced) when:

- The working tree diverges from the snapshot for reasons other than transient markers — for example the user typed a fresh prompt after the cancel. The recovery would silently commit unintended drift, so it refuses.
- The cycle phase is not `WriteApplied` / `Committed` — preflight does not treat earlier phases as written-but-uncommitted.
- `git::commit` itself errors out (lock contention, broken submodule pointer, malformed snapshot). The error is logged and the preceding drift hint is shown.

In those cases the documented manual repair is `agent-doc write --commit <FILE>`; if that command also errors, fall back to `agent-doc commit <FILE>` to land the snapshot through the binary-owned boundary.

## Compact Exchange

`agent-doc compact <FILE> --component exchange --commit` uses the shared convergence authority rather than a whole-document response payload:

- Compact Exchange does not emit editor IPC `fullContent` or ask JetBrains to replace the whole visible buffer.
- The binary computes the compacted document, then uses the quiescent CRDT delivery contract above when a replica is attached. A reliable legacy endpoint without an attached model may still receive a component `op:replace`; the guarded direct-write path (`source=compact_exchange_direct_write`) is only the detached fallback, so the session document is replaced behind the editor only when no plugin is running and disk still matches the expected source content.
- When the JetBrains sidecar reports a stale live buffer/stale file cache, or an active listener cannot prove the editor patch via ack-content, compact fails closed before replacing the document or advancing the snapshot. Expected ops-log signatures include `visible_write_deferred_live_buffer_changed source=compact_exchange_direct_write` and `compact_writeback ... transport=blocked reason=... action=refuse_external_disk_write`.
- Recovery is to resolve the IDE buffer first: save, discard, or reload the document so JetBrains and disk agree, then rerun `agent-doc compact <FILE> --component exchange --commit`.

Do not use `agent-doc write --commit <FILE>` for this Compact Exchange guard failure unless there is also a normal response patch that already landed in the working tree. The stale-buffer / blocked-editor compact guard is intentionally pre-write; the correct recovery is to make the visible editor state current, let the plugin apply/prove the patch, and rerun compact.

After a successful compact commit, the binary writes the VCS refresh signal when that channel exists. The JetBrains plugin should refresh VFS/VCS from that signal so gutter state and file cache match the committed document. If a later normal `finalize` still surfaces a File Cache Conflict dialog, use the Dialog Contract and Recovery sections above.

## Plugin-Side Notes

The JetBrains plugin refuses to mutate an open document while IntelliJ has a pending File Cache Conflict for that file. Conflict detection is terminal for that IPC payload:

- socket IPC returns failure so the binary can retain the response and retry through the normal closeout path;
- file-watch IPC records `file_cache_conflict_pending`, deletes the queued patch file, and leaves the response for binary-owned retry rather than replaying the old payload later;
- the plugin refreshes `VisualHighlighterManager` on the blocked path so shared `agent_doc_visual_tokens_json` ranges do not remain stranded in the pre-conflict state.

The plugin must not keep a conflict-deferred patch id, wait for the dialog to resolve, or apply the old payload after the user chooses either **Accept / Load FS changes** or **Cancel / Keep memory changes**.

The binary-side auto-recovery above remains the defense-in-depth path for older plugin versions or any case where the response had already reached the working tree before the refusal.

## Late-Accept Replay (`#jbccacceptdup`)

Historically, when the user left the File Cache Conflict dialog open past the IPC ack window and accepted it **after** the cycle had already reached `commit_success`, the plugin still had a deferred IPC payload queued for the now-stale cycle. Accepting replayed that payload on top of the committed working tree, producing a second `### Re: …` block that duplicated the response already in HEAD. The next `agent-doc preflight` then drift-recovered and auto-committed the duplicated state.

Deterministic SimWorld coverage for this branch lives in `src/agent-doc/src/sim_world.rs`:

- `jb_cache_conflict_accept_late_replays_duplicate_response_today` — failing baseline: post-commit replay yields two `### Re:` blocks with `snapshot` still pinned to the original commit.
- `jb_cache_conflict_accept_late_replay_manual_repair_recovers_today` — documented manual recovery: operator removes the replayed block and re-commits so the snapshot tracks the cleaned working tree (`dedupe_responses` only handles identical-body duplicates, so the operator-edit path is the safe baseline).

The current plugin-side fix is simpler than the original plan: pending File Cache Conflict blocks do not queue deferred payloads, so late Accept has no old response patch to replay. The historical SimWorld scenarios remain useful as regression references for older plugin versions.

## See Also

- `runbooks/commit.md` — overall closeout / repair ordering.
- `runbooks/baseline-drift.md` — manual-commit baseline drift and preserve-session reset.
- `tasks/agent-doc/plan-jb-cache-cancel-stuck-cycle.md` — full plan with phases 1–5 and the deterministic SimWorld scenarios.
- `tasks/agent-doc/plan-jb-cache-conflict-accept-duplicates-response.md` — `#jbccacceptdup` fix plan (late-accept replay).
