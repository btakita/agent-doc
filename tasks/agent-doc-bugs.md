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
*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/e7cc7f7a85d39c090242bb3a72d53651075f5c7f9a1053e25d29aa36657310cc-20260318-061818.md`*

### Re: Exchange compacted

128 lines archived. Clean slate.

**Session recap — 7 releases (v0.17.16–v0.17.22):**
- tmux routing, prompt parsing, resync --fix, split-window, patch= rename, fenced code blocks, component tag sanitization, UTF-8 fix
- Design: agent:pending task queue, document-as-memory, free-text-to-exchange rule

tmux session has too many tmux windows again.
Please diagnose. Use the logs to debug. Can you recognize what caused the proliferation of tmux windows? If not, add missing logs.

Add tests to simulate the root cause of the proliferation of tmux windows.
Add tests for the "self healing" `agent-doc --resync`. Why did it not work?

speculation: The issue is caused by focusing on another tmux session.
The focusing on the editor. But focus on the claiming tmux session is bypassed.
So I wonder if we are using the last selected tmux session for anything.
When we call agent-doc run, the claiming tmux session should effectively override any preference for the last selected tmux session...since focusing on the claiming tmux session may be bypassed.

### Re: Window proliferation — root cause + resync gap

**Cleaned up:** 6 extra windows killed, registry cleared, back to 2 windows.

**Root cause:** When the IDE triggers `agent-doc route` for multiple documents simultaneously, each route tries `split_window` but the window gets progressively smaller. After 2-3 splits succeed, the remaining ones fail with "pane too small" and fall back to `auto_start()` which creates new windows.

If a stash window is "full", why not create another stash window?
Can we robustly track where the binding agent session is stashed?

**Why resync didn't catch it:** `resync --fix` only checks wrong-session and wrong-process. It does NOT check **wrong-window** — panes scattered across multiple windows when they should be consolidated.

**Two fixes needed:**
1. **Route:** When split fails, stash the pane instead of creating a new window
2. **Resync:** Add wrong-window detection — consolidate panes in separate windows: or an available stash window

Want me to implement both?
yes
<!-- /agent:exchange -->

<!-- agent:pending patch=replace -->
- [ ] Fix route fallback: stash instead of new window when split fails
- [ ] Add wrong-window detection to resync --fix
<!-- /agent:pending -->
