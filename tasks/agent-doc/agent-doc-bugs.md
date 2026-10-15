---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: ba846c4c-2e01-4e12-9539-096faec207ee
  codex: 01a13cac-54da-7403-9456-6fc418818870
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
presets:
  '#actionable-review': Add actionable review items into backlog + queue
  '#agent-doc-causes': Fix agent-doc causes
  '#do-backlog': /goal do backlog items
  '#gh-fix': fix then close
  '#release': release + publish
  '#self': agent-doc-bugs.md
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: go
---

## Status

<!-- agent:status patch=replace -->
Review pass done. frontendlivedispatch closed on live evidence. Two review items remain, both genuinely gated: an operator decision (jb262dynlauncherrecommen) and an external PyPI reply (pypiquotarecheck).
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append subagents -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261015-071654.md`*

Compacted content:
- Archived 16 response topic(s): dispatched #gh-fix 261–264 + release batch; Subagent completed; Subagent completed; 13 more
- Prior summary/context: prior compacted content: Archived 4 response topic(s): agent-doc fixes, release, and tmux auto-sync; GitHub issue 259...

### Re: queue — #gh-fix GH #266 dispatched; release + publish waits — opus-5-5 · 2026-10-15T03:21-04:00

**GH #266** (Clear Session Context reports every Claude Code `/clear` as "not proven submitted") is claimed for `subagent:gh266` and dispatched to a background subagent, in its own worktree `~/work/btakita/agent-doc-wt/gh266` on branch `fix/gh266-claude-clear-proof` cut from `origin/main`.

Here is what the issue says is wrong, and what the subagent was asked to fix:
- `context_clear_history_proves_cleared_state` refuses once the scrollback passes 200 lines. Claude Code's `/clear` leaves the scrollback in place, so for Claude Code panes the fallback in `upgrade_unobserved_clear_from_pane_history` can never pass on a real session. The `fresh_chat_visible` proof (`#clearfreshchatproof`) is gated to Codex only.
- The fix is a Claude Code cleared-state proof that looks only at the pane tail after the `/clear` line. It must still fail closed when the old conversation is still visible.
- The probe should resolve the harness the same way the supervisor does, so a Claude Code pane is not probed as `harness=default`. The subagent will also look at the `harness_queued_input` misfire.
- It will update the spec, add regression tests, run `make check`, and commit and push the branch. Integrating it, `make install`, and closing #266 stay with the coordinator.

> **Queue prompt:** release + publish

**Deferred:** this release has to include the #266 fix, so it waits until the subagent reports and the branch is merged and installed. The next queue cycle cuts it.
<!-- agent:boundary:77c3a41f -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built
<!-- agent:queue subagents preset="#spec-test-build-install-commit-push" priority go -->
- #gh-fix https://github.com/btakita/agent-doc/issues/266
- release + publish
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#jb262dynlauncherrecommen] [recommended] [operator-verify] [agent-doc] Operator decision: jb262dynupgrade (153c72916) chose the accurate-warning option (a 262 package reports declined_by=modular_package and falls back to file replacement + restart). Either keep that as the supported policy (then close this), or authorize a real restart-free launcher for the exact-262 split-mode plugin that swaps backend + frontend (the frontend may run in a separate JetBrains Client process), proven against a live 262 IDE.
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically by a human reply on pypi/support#12214, or make pypi-quota-check reporting a ceiling in reach. Rechecked 2026-10-12: pre-cutover 131 releases 8.361 GiB (0.35.113..0.35.343), post-cutover 91 releases 3.19 MiB (mean 35.9 KiB), about 47,722 further releases fit; VERDICT headroom fine. The support issue is still open with one comment, unchanged since 2026-09-10. Nothing to act on or delete. If the grant lands, record it and drop this item; deletion stays reserved for a ceiling the authenticated check reports.
<!-- /agent:review -->

## Todo

<!-- agent:todo -->
- [ ] Next Remote Dev session: confirm tab switches move tmux (GH #262 fix, plugin 263 0.2.617 after v0.35.504 is installed on the backend)
<!-- /agent:todo -->

<!-- agent:icebox -->
- [ ] [#ghrecvpolicy] [agent-doc] Recipient-side [receive] auto_create_components policy for multi-user workspaces (GH #40, https://github.com/btakita/agent-doc/issues/40). Parked until multi-user workspace support exists.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
