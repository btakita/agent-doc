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
   - Archives the original content to `.agent-doc/archives/`
   - Replaces exchange content with the supplied summary, or when no custom `--message` is provided, with a default session summary that includes archive pointer plus live backlog/queue/icebox context
   - Updates snapshot atomically
   - Closes out via the binary-owned `agent-doc commit` path and verifies the VCS refresh signal when available
   - Uses component `op:replace` editor IPC for template exchange compaction when a listener is active, never editor IPC `fullContent`; the guarded direct-write path is only the no-listener fallback, so a whole-document editor replacement cannot race the next prompt being drafted and the binary does not write behind a running plugin
   - `#jb-compact-editor-buffer-flush`: the `op:replace` convergence updates only the live editor's in-memory buffer — the plugin does not save. Before the `--commit` selective commit stages the snapshot, the binary asks the editor to flush its buffer to disk (the same `save_document` IPC preflight uses for `live_prompt_drift`) so the working-tree file holds the compacted content. Without this flush the commit compares a stale pre-compact working tree against the compacted snapshot, treats the snapshot as historical exchange drift, and repairs it back to HEAD — leaving HEAD and disk pre-compact (the "Compact Exchange left an uncommitted summary" defect). Fail-open: `commit_compacted_authoritative` still verifies HEAD landed the compacted content and fails closed otherwise. Look for `compact_editor_buffer_flush ... transport=save_document` in `ops.log`.
   - `#jb-compact-commit-stale-relay-canonical`: when the compaction writes through the **stale-lease disk-authority path** (`crdt_cpc_write_disk_authority_stale_lease`, `live_editors == 0` under a phantom editor lease — e.g. an older plugin whose CRDT replica register failed, so no live replica is registered while the reactive open-docs projection still reports the editor open), only disk + snapshot hold the compacted content; the lazily relay canonical stays **frozen** at the pre-compact text. The selective commit then resolves its document content through the realtime authority (`try_resolve_current_document_content`), which keeps editor authority for the phantom lease and returns that frozen pre-compact canonical, so the commit lands pre-compact content in HEAD (`compact_commit_head_mismatch`, observed live on `agent-doc-bugs2.md`). Fix: `commit_compacted_authoritative` converges the lazily canonical to the authoritative compacted content (`agent_doc_crdt_relay_io::adopt_authoritative_text_for_file`) **before** the commit reads it — the reliable-sync plane is authority, so the compacted content must reach the plane, not only the disk/snapshot durability sidecars. Authority-gated + fail-open: a headless/missing-relay document (`crdt_adopt_authoritative_text_deferred`) leaves the disk+snapshot write authoritative, and `verify_compact_head_landed` still fails closed if HEAD does not land the compacted content. Look for `crdt_adopt_authoritative_text ... changed=true` in `ops.log`.
   - If JetBrains reports a stale live buffer/stale file cache, or an active listener cannot prove the editor patch, compact fails closed before replacing the document or advancing the snapshot. Look for `visible_write_deferred_live_buffer_changed source=compact_exchange_direct_write` or `compact_writeback ... transport=blocked reason=... action=refuse_external_disk_write`, resolve the IDE buffer/listener by saving, discarding, reloading, or retrying the plugin path, then rerun the same compact command.
   - If a live template/CRDT session already has a bare `compact exchange` user directive in the diff, later write-back also forces `agent:exchange` replacement semantics so the checkpoint summary cannot silently append over old exchange content
   - Pass `--message "summary text"` to include a custom summary instead of the binary default session summary

See [jb-cache-conflict.md](jb-cache-conflict.md) for the JetBrains File Cache Conflict split between normal response IPC recovery, Compact Exchange editor convergence, and the no-listener guarded direct-write fallback.
