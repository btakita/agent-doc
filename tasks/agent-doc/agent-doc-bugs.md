---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
agent: claude
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  "#spec-test-commit-push": update spec + tests. commit + push

---

## Status

<!-- agent:status patch=replace -->
✅ Fixed the frontmatter body-slice bug in `src/agent-doc/src/frontmatter.rs` so repeated frontmatter rewrites no longer accumulate blank lines before the first body heading. Added regression coverage for exact body preservation across repeated parse/write cycles, then verified the crate with `cargo test`, `cargo build --release`, and `cargo install --path .`. The code change is committed and pushed in `src/agent-doc` as `f4b9f59` on `20260422/codex-agent-backend`.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Compacted exchange. Key work completed: frontmatter prompt presets (#fppr) implemented with round-trip, validation, and orchestrate expansion; orchestrate --plan dry-run mode (#0z3v); guard marker strip-before-commit (Option B); orchestrate subprocess fixes — frontmatter args forwarding (#orch1), stderr surfacing (#orch2), prompt prefix stripping (#orch3), --verbose for streaming (#orch4); (HEAD) marker preservation in working tree (#hfix1-4) with split convention (snapshot strips, working tree preserves); JB plugin preserve_head IPC (#hfix5, v0.2.76); VS Code preserve_head IPC (#hfix6, v0.2.9, commit 8202f80); Editor SPEC.md documented preserve_head + FFI variant table (#hfix7); adjacent code fence dedup bug fixed. Discovery: orchestrate --from-exchange stale-task parsing regression persists after #orch3 — parser scans full exchange and .last() picks wrong block when multiple orchestra directives exist in compacted+appended history. Needs fix: scope to latest unprefixed orchestra block. Open: #hfix8 gated on user manual verify, #ha3h gated on multi-user prerequisite.
<!-- agent:boundary:0006dfcc -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [/] [#hfix8] Bump JB plugin version, build + install, manual verify (HEAD) appears after patchback with IPC listener active
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Blocked until multi-user workspace support exists; issue #40 explicitly says not to ship this before that prerequisite. Tracked: https://github.com/btakita/agent-doc/issues/40
<!-- /agent:pending -->

<!--
- [ ] [#pdar] [recommended] Evaluate optional binary auto-resolution only for the narrow case where the prompt is explicitly do #id and there is exactly one matching open pending item
-->
