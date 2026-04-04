---
agent_doc_session: 92fba2ac-9d1f-4a7a-8764-821a8a08ecd0
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
claude_args: "--dangerously-skip-permissions"
---

<!-- agent:status mode=replace -->
- [x] Phase 1
<!-- /agent:status -->

<!-- agent:exchange mode=append -->
*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/e7cc7f7a85d39c090242bb3a72d53651075f5c7f9a1053e25d29aa36657310cc-20260319-044317.md`*
### Re: Exchange compacted

127 lines archived. Clean slate.

**Session recap — 13 releases (v0.17.16-v0.17.28):**
- Tmux routing, prompt parsing, resync --fix, split-window, stash overflow
- patch= rename, pending component, component tag sanitization, UTF-8 fix
- pulldown-cmark AST, diff --wait, agent:input support
- JB/VSCode plugin code-block fix (0.2.4/0.2.2)
- tmux_session validation (warns + falls back for non-existent sessions)

**Key bugs fixed this session:**
- Window proliferation: `tmux_session: '23'` pointed to non-existent session → silent creation loop
- Stale binary: `~/.cargo/bin/agent-doc` was v0.17.16 while `.bin/agent-doc` was v0.17.28 → tmux-spawned sessions used old binary
- Dead pane cleanup: killed non-responsive pane %82

**Process improvements:**
- Release checklist now includes `cargo install --path .` as final step
- VERSIONS.md backfilled through v0.17.28
- `/dev` skill flagged for future release orchestration
<!-- agent:boundary:9f55e705 -->
<!-- /agent:exchange -->

<!-- agent:pending patch=replace -->

<!-- /agent:pending -->
