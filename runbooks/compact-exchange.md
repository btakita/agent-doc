# Compact Exchange

Steps to compact an agent-doc exchange component when it grows too large.

## When to compact

- User explicitly requests "compact exchange"
- Never auto-compact without user approval
- Auto-compact is off by default. The only way it becomes "active" is an explicit `agent_doc_auto_compact` opt-in in document frontmatter or project `.agent-doc/config.toml`. Without that opt-in, `session_accretion.guidance` emits a gated reminder ("Exchange is large; ask the user before compacting...") instead of an imperative `Run agent-doc compact <FILE>` directive — agents must follow that gate and ask the user before invoking compact.

## Steps

1. **Read the full exchange content** from the document

2. **Read the canonical state surfaces** that must survive compaction:
   - `agent:backlog` / `agent:pending`
   - `agent:queue`
   - `agent:icebox`
   - Treat those components as the source of truth for unresolved work. Do not rely on old exchange prose alone when deciding what context must survive.

3. **Summarize** — preserve:
   - Decisions made (with rationale)
   - Key facts and discoveries
   - Open items and pending work
   - The top active backlog items, including gated items that still matter
   - The queue head and any remaining queued prompts when present
   - Iceboxed work that still affects future routing or context
   - **Unanswered user input** — if the exchange contains uncommitted questions or instructions that haven't been responded to, note them as open items in the summary (don't silently drop them)
   - Use frontmatter `prompt_presets` only as optional policy knobs for how much state to mention (for example "include the top 3 backlog items"), not as the source of substantive context
   - Discard verbose back-and-forth, code snippets already committed, exploratory dead-ends

4. **Run `agent-doc compact <FILE> --component exchange --commit`**
- The CLI, JetBrains action, and VS Code action submit one operation to the CPC;
  they never compute or apply compaction themselves. The CPC performs the read,
  archive, CRDT replacement, projection ACK, and commit in one controller-owned
  mutation scope.
- Inside that scope, relay reads/writes and the commit use in-process controller
  state rather than recursively requesting the controller socket. This removes
  the short generic RPC timeout and self-queue deadlock from Compact Exchange.
   - Archives the original content to `.agent-doc/archives/`
   - Replaces exchange content with the supplied summary, or when no custom `--message` is provided, with a default session summary that includes archive pointer plus live backlog/queue/icebox context
   - Updates snapshot atomically
   - Closes out via the binary-owned `agent-doc commit` path and verifies the VCS refresh signal when available
   - Uses the quiescent CRDT-only convergence path when a replica is attached; component `op:replace` remains only for a reliable legacy endpoint without an attached model, and editor IPC `fullContent` is never used. The guarded direct-write path is only the detached fallback, so a second mutation cannot race the CRDT delivery or the next prompt being drafted and the binary does not write behind a running plugin
   - `#jb-compact-editor-buffer-flush`: the `op:replace` convergence updates only the live editor's in-memory buffer — the plugin does not save. Before the `--commit` selective commit stages the snapshot, the binary asks the editor to flush its buffer to disk (the same `save_document` IPC preflight uses for `live_prompt_drift`) and waits for disk to match the **live compacted target**. That target includes any unresolved post-boundary prompt, while the separate committed snapshot intentionally omits it. Look for `compact_editor_buffer_flush ... transport=save_document` in `ops.log`.
   - `#jb-compact-two-target-lineage`: when live and committed targets differ, closeout never adopts the committed-only target into an already-converged relay. Doing so schedules a whole-buffer editor rebootstrap and lets delayed JetBrains document events replay the pre-compact lineage, resurrecting archived content as uncommitted text. The commit stages/verifies the committed target without changing the live target.
   - `#jb-compact-commit-stale-relay-canonical`: when the compaction writes through the **stale-lease disk-authority path** (`crdt_cpc_write_disk_authority_stale_lease`, `live_editors == 0` under a phantom editor lease), disk + snapshot become compacted but the lazily relay canonical can remain frozen at the pre-compact text. Before commit, compact repairs that stale fallback to the **live** compacted target. A differing canonical with a registered live editor is concurrent operator drift and fails closed instead of being overwritten; the post-commit HEAD check still verifies the committed target.
   - `#cpc-commit`: when a live editor owns the document, the CLI is a non-authoritative relay replica and the commit is delegated to the CPC controller — the authoritative owner of the converged relay canonical — which commits IN-PROCESS where its canonical is authority. This is what lets `Compact Exchange` land from a session with the document open in the IDE instead of failing closed with `editor is the current authority ... was not used as commit authority`. `commit_with_outcome` calls `commit_document_via_controller`: headless returns `Ok(None)` (local commit as before); an editor-attached document with a reachable controller sends the `commit_document` RPC (`handle_commit_document_rpc` flushes editor ops into the canonical, then commits via the `ProjectControllerRuntimeEffects::commit_document` port). Delegation is skipped under `--force-disk` and only targets an already-running controller; any error falls through to the local commit. Look for `controller_commit_document ... barrier_ready=` in `ops.log`.
   - `#cpc-commit-local-read`: once the controller-owned commit crosses that relay barrier, current-document reads use the controller's in-process canonical instead of queuing socket requests back into the same controller. The normal authority resolver remains the fallback if the local relay is unexpectedly unavailable.
   - The JetBrains `Compact Exchange` action immediately overlays `⟳ agent-doc: Compacting Exchange` on the editor-top turn banner and keeps it visible until the asynchronous command completes. Operation tokens prevent an older completion callback from hiding a newer action.
   - If JetBrains reports a stale live buffer/stale file cache, or an active listener cannot prove the editor patch, compact fails closed before replacing the document or advancing the snapshot. Look for `visible_write_deferred_live_buffer_changed source=compact_exchange_direct_write` or `compact_writeback ... transport=blocked reason=... action=refuse_external_disk_write`, resolve the IDE buffer/listener by saving, discarding, reloading, or retrying the plugin path, then rerun the same compact command.
   - If a live template/CRDT session already has a bare `compact exchange` user directive in the diff, later write-back also forces `agent:exchange` replacement semantics so the checkpoint summary cannot silently append over old exchange content
   - Pass `--message "summary text"` to include a custom summary instead of the binary default session summary

See [jb-cache-conflict.md](jb-cache-conflict.md) for the JetBrains File Cache Conflict split between normal response IPC recovery, Compact Exchange editor convergence, and the no-listener guarded direct-write fallback.
