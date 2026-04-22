---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---


## Status

<!-- agent:status patch=replace -->
✅ agent-doc v0.32.1 — env frontmatter wired into all Claude-spawning paths (start / run / stream / parallel). Build clean, 42 targeted tests passing. Ready for live test.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
v0.33.8–0.33.11 bug session. Closed: #a002 (normalize telemetry code-complete), #y2gr (skill install runbooks sync fixed in v0.33.7). Added: future-work signal promotion to SKILL.md + pending-ops.md; future-work lint in binary (#7x90, write.rs). Fixed #v30f: file rename → stale pane — sync.rs reuses pane on rename, JB FileRenameListener.kt + VS Code onDidRenameFiles handler, specs updated, 5+6 tests. IPC patchback postmortem: 3 stacked bugs (IPC path resolution, bool→i32 FFI ABI, stale .so from cargo install not running lib-install) — all fixed over 7 turns. #qjc8: lift_pending_from_exchange() auto-repair pass in write.rs + 5 tests. #3vk0: lib-install atomic rename (temp+rename vs in-place copy) to prevent IDEA mmap corruption on same-version reinstall + inode test. Released v0.33.11: atomic lib-install, resync test timing, route test env-var mutex. Pending: #ha3h (recipient policy, gated/multi-user), #066r (Opus 4.7 test).
<!-- agent:boundary:42c4b9eb -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Ship with multi-user support, not before. Tracked: https://github.com/btakita/agent-doc/issues/40
- [ ] [#066r] Test on Opus 4.7 — baseline current behavior before prompt changes, then iterate
<!-- /agent:pending -->
## Assistant
