---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: ba846c4c-2e01-4e12-9539-096faec207ee
  codex: 01a148af-f3c1-78c0-92b7-988ce7453d7c
agent: codex
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
---

## Status

<!-- agent:status patch=replace -->
Review pass done. frontendlivedispatch closed on live evidence. Two review items remain, both genuinely gated: an operator decision (jb262dynlauncherrecommen) and an external PyPI reply (pypiquotarecheck).
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append subagents -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261017-070840.md`*

Compacted content:
- Archived 26 response topic(s): unwedging docs.md; Subagent completed: `subagent:todoidlayer` returned work for queue item “todo ids opportunistically layer on existing...; Subagent completed: `subagent:gh276` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issue...; 23 more
- Prior summary/context: prior compacted content: Archived 33 response topic(s): GH #268–#273 + queue duplication; GH #268–#273 + queue duplic...

### Re: Queue-prompt flood and GH #282–#283 — gpt-5 · 2026-10-17T03:55-04:00

> **Queue prompt:** Fix agent-doc causes of **Queue prompt:** sdk-dev.md issue:

This was already fixed on current `origin/main`. Commits `143e73cff0` and `c4cf08336c` are ancestors of the current base, the focused queue-prompt regression suite passed 7/7, and the current `sdk/sdk-dev.md` has four distinct answered queue echoes, no repeated `❯ > **Queue prompt:**` flood, and an empty queue. No additional product change was needed.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/282

Fixed and closed. Commit `a7f64f7699` (`fix(jetbrains): stop retrying plain markdown replicas`) makes plain Markdown a terminal non-replica state, clears registration retry state, advances the shadow, and permits later marker-bearing text to be reclassified. Targeted tests and the complete JetBrains test/build passed; the worker branch was pushed and issue #282 is closed.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/283

Fixed and closed. Commit `bbc7a53bed` (`fix(jetbrains): preserve Remote Dev focus evidence`) carries Remote Dev controller-selection evidence through the generation-fenced publication gate, adds named INFO-level outcomes and Throwable logging, and covers both the classic/263 listener and the 262 backend fallback. The full worker suite and final affected reruns passed; the worker branch was pushed and issue #283 is closed.

The two fixes were integrated serially as `0e597981a8` and `90e238ce69`, followed by the one batch generation commit `25f3bd933b` (classic `0.2.675`, JetBrains 262 `0.2.677`, JetBrains 263 `0.2.676`). The authoritative integrated `make check` passed with exit 0 and zero failures. The single `make install` passed and installed the binary/library, skills, and editor packages. Main CI run `39011286916` was red only in the pre-existing `queue_auto_clear_types_clear_between_items_into_the_same_harness` tmux case on base commit `f9cd1ad325`; the required local `make tmux-ci` reproduction passed with exit 0 and zero failures. The integration worktree is clean and the remote main tip is still its verified fast-forward base.
> **Chat prompt (#chatprompt):** published?

### Re: Publication status — gpt-5 · 2026-10-17T13:40-04:00

Not as a new release. The fixes are pushed to `main` at `25f3bd933b`, and CI run `39015481373` passed. The latest published release is still `v0.35.515`, whose tag (`4c4f32d596`) predates these fixes, so they are not yet in a GitHub/PyPI release.
<!-- agent:boundary:f163b149 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built
<!-- agent:queue subagents preset="#spec-test-build-install-commit-push" priority -->
<!-- /agent:queue -->

## Todo

<!-- agent:todo -->
- [ ] [#ahfe] Live-check GH #267 on the 263 IDE: after `agent-doc upgrade` WITHOUT restart, once idea.log shows `[native] hot-reloaded libagent_doc`, Run Agent Doc should complete (ops.log `editor_route_superseded_editor_admitted`, route output ends with the advisory line). 2026-10-15 operator: did not work (believed); watch for bug reports before reopening GH #267.
- [x] [#gq2c] After the haiven-dev supervisor recycles onto the new build, re-run Agent Doc on tasks/sdk.md: bracketed todo ids are admitted again (it currently uses bare ids as a workaround).
- [ ] [#ctg7] Under heavy load, switch the editor 2 panes -> 1 -> 2 and confirm tmux follows and stays at 2 (stale-controller fix).
- [ ] [#n7kq] In the sdk.md session: restore the 7 todo ids the 23:30 --todo-edit dropped (#tdclireview, #tdcontracts162, #tdpromotioncallers, #tdinspectclones, #tdfpe202, #tdadr0033, #tdadr0030) with --todo-edit "<exact text>=#tdX <same text>", or accept the ids the preflight backfill assigns
- [ ] [#8ph6] In the docs.md session: check off the Google Drive re-auth todo (--todo-done httpsdocsgoogle once backfilled, or --todo-done with its exact text)
- [ ] [#t1v0] In the sdk.md pane, press Clear Session Context again: the 06:03 presses were held behind the open cycle and then dropped by the install restart. Check first that sdk.md's queue has not opened its tdue4cpuset cycle.
<!-- /agent:todo -->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#jb262dynlauncherrecommen] [recommended] [operator-verify] [agent-doc] Operator decision: jb262dynupgrade (153c72916) chose the accurate-warning option (a 262 package reports declined_by=modular_package and falls back to file replacement + restart). Either keep that as the supported policy (then close this), or authorize a real restart-free launcher for the exact-262 split-mode plugin that swaps backend + frontend (the frontend may run in a separate JetBrains Client process), proven against a live 262 IDE.
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically by a human reply on pypi/support#12214, or make pypi-quota-check reporting a ceiling in reach. Rechecked 2026-10-12: pre-cutover 131 releases 8.361 GiB (0.35.113..0.35.343), post-cutover 91 releases 3.19 MiB (mean 35.9 KiB), about 47,722 further releases fit; VERDICT headroom fine. The support issue is still open with one comment, unchanged since 2026-09-10. Nothing to act on or delete. If the grant lands, record it and drop this item; deletion stays reserved for a ceiling the authenticated check reports.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#ghrecvpolicy] [agent-doc] Recipient-side [receive] auto_create_components policy for multi-user workspaces (GH #40, https://github.com/btakita/agent-doc/issues/40). Parked until multi-user workspace support exists.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
