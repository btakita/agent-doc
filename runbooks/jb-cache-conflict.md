# JetBrains File Cache Conflict

When IntelliJ has a session document open and the binary writes through IPC, the IDE occasionally surfaces a **File Cache Conflict** dialog because its cache disagrees with what is about to land on disk. The dialog gives the user two paths.

## Dialog Contract

- **Accept / Load FS changes** — IntelliJ resolves its cache state, but the plugin must not replay a payload that was already blocked by conflict detection. The binary-owned retry path keeps responsibility for the response.
- **Cancel / Keep memory changes** — IntelliJ preserves the visible editor buffer. The plugin must not write over that memory state; it has already signaled failure so the binary can recover or fail closed. Historically the working tree could be left with a partial write while the cycle stayed at `WriteApplied` — see "Recovery" below.

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

`agent-doc compact <FILE> --component exchange --commit` uses a different closeout path from normal response IPC:

- Compact Exchange does not emit editor IPC `fullContent` or ask JetBrains to replace the whole visible buffer.
- The binary computes the compacted document, then tries component `op:replace` editor IPC when a listener is active. The guarded direct-write path (`source=compact_exchange_direct_write`) is only the no-listener fallback, so the session document is replaced behind the editor only when no plugin is running and disk still matches the expected source content.
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
