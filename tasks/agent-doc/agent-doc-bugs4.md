---
agent_doc_session: b7e2a8c1-3f5d-4e9a-8b1c-6d4f2e8a9c3b
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->

### Re: figma-diagrams patchback + lost pane — opus-4-6

Investigated both issues via the ops log, session log, cycle state, and captures for `src/session-share`.

#### Patchback status

Both responses in `tasks/figma-diagrams.md` **were** successfully patched back and committed during the original session in pane %138:

- **Cycle 1** (20:11 UTC): "Re: Mermaid-to-Figma migration plan" — `commit_success`, capture shows `state: "committed"`
- **Cycle 2** (20:19 UTC): "Re: Figma MCP setup" — `commit_success`, capture shows `state: "committed"`

If you wrote a **third** prompt after cycle 2 and Claude started responding in the console, that response would have been lost when the pane died — there's no third capture or cycle record, meaning `finalize` never ran for it. The Codex Stop-hook backstop only fires if the hook is wired and the session is still alive; a killed pane bypasses it entirely.

#### Why the pane was lost

Session log:
```
[1777234179] session_start pane=%138  ← original
[1777234896] session_start pane=%140  ← replacement (12 min later)
```

The `agent-doc` pane lifecycle has three relevant mechanisms:

1. **`sync` layout reconciliation** — when the editor switches focus, `agent-doc sync` runs and can stash or kill panes that don't match the current layout
2. **`route` auto-start** — if the registered pane (%138) is found dead, `resolve_or_create_pane()` provisions a new one (%140) and calls `evict_previous_stash_pane_entry()` to clean up
3. **`sessions.json` single-pane invariant** — registration removes stale entries pointing to the same pane

Most likely: when you navigated away, either (a) `sync` killed %138 as part of layout reconciliation (moving it to stash where it was later evicted), or (b) the Claude Code process in %138 exited on its own (timeout/idle), and when you navigated back, `route` found it dead and auto-started %140.

The net result: if Claude was mid-response in %138 when it died, `finalize` never ran and the response was lost. The capture/recovery mechanism only works if the response was captured *before* the pane died.

#### Bugs to track

1. **Pane killed on editor navigation** — `sync` should not kill a pane that has an active Claude session with uncommitted work. Needs a "busy" check before eviction.
2. **No capture for in-flight responses** — if Claude is mid-generation when the pane dies, there's no safety net. The Stop hook can't fire on a dead process. A periodic checkpoint capture (every N seconds during response generation) would prevent total loss.
<!-- agent:boundary:4f137dac -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#257p] `#oauth` Skill fails to persist response when an MCP OAuth flow interrupts the cycle — the authenticate tool call consumes the turn and the skill never reaches `agent-doc finalize`. Observed in `tasks/figma-diagrams.md` session 2026-04-26: user asked for Figma MCP help, skill called `mcp__plugin_figma_figma__authenticate`, responded in console, but never ran finalize. **Root cause:** no guard or reminder in the skill workflow for tool calls that break the normal respond→finalize flow. **Plan:** the skill should either (a) treat MCP auth as a sub-step and still finalize after, or (b) the binary should detect that a response was generated but finalize was never called and warn/recover.
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->
