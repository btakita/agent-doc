---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: ba846c4c-2e01-4e12-9539-096faec207ee
  codex: 01a1545a-f42c-7aa1-b9a0-aeafeba96910
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
presets:
  '#agent-doc-causes': Fix agent-doc causes
  '#audit': 'Audit agent-doc runtime for dogfooding. Add issues to the backlog + queue.'
  '#self': agent-doc-bugs.md
dogfood:
  path: tasks/agent-doc/agent-doc-bugs.md
  auto: true
queue: go
---

## Status

<!-- agent:status patch=replace sync="Track the latest published agent-doc release + publication time and latest JetBrains plugin release version + publication time." -->

📥 Queue handoff: 4 reported items awaiting coordinator integration, verification, and closeout.
Review pass done. frontendlivedispatch closed on live evidence. Two review items remain, both genuinely gated: an operator decision (jb262dynlauncherrecommen) and an external PyPI reply (pypiquotarecheck).
2
<!-- status:sync -->
Review pass done. frontendlivedispatch closed on live evidence. Two review items remain, both genuinely gated: an operator decision (jb262dynlauncherrecommen) and an external PyPI reply (pypiquotarecheck).

Latest published release: [v0.35.528](https://github.com/btakita/agent-doc/releases/tag/v0.35.528) (GH #320 option A: Remote Dev split state reaches the controller; background-drain busy-turn defer, orphan-drain claim re-read, state-propagation spec), published on GitHub at 2026-10-20 07:37:15 UTC (15 assets); PyPI 0.35.528 followed at 2026-10-20 07:38:01 UTC; installed locally (agent-doc 0.35.528). JetBrains packages in v0.35.528: 0.2.723 (classic), 0.2.725 (IDE 262), 0.2.724 (IDE 263), published with the release at 2026-10-20 07:37:15 UTC; installed: IntelliJ IDEA 2026.1 classic 0.2.723, Rider 2026.2 IDE-262 0.2.725. VS Code extension 0.2.83.
<!-- /status:sync -->
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append subagents -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261020-142741.md`*

Compacted content:
- Archived 49 response topic(s): Subagent completed: `subagent:issue310` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/is...; Subagent completed: `subagent:archivepath` returned work for queue item “re: `agent:done` `archive` relative path. We...; Subagent completed: `subagent:issue309` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/is...; 46 more
- Prior summary/context: prior compacted content: Archived 75 response topic(s): Queue-prompt flood and GH #282–#283; Publication status; rele...
> **Chat prompt (#chatprompt):** subagent: Fix the agent-doc causes of the nested status:sync. Use AST parsing to fix.

> **Queue prompt:** There was 1 tmux pane with 2 editor panes.

> **Queue prompt:** route: a background orphan-drain dispatch types into a pane right after reading its busy-turn cue (route_dispatch_only_busy_active_turn_skip_wait then tmux_text_enter; from GH 319)

> **Queue prompt:** investigate: preset head upgrade was dispatched 2 s after its claim landed (GH 319 "also seen"; likely claim-visibility timing in the controller state.db read)

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/320

> **Queue prompt:** steering: the agent's own committed `> **Queue prompt:** <line>` echo blockquotes in agent:exchange were surfaced to the same turn as 5 NEW operator exchange prompts (source=exchange change=added dispatch=subagent) right after closeout on 0.35.527; agent-authored response cells must never be read back as operator steering

> **Queue prompt:** release + publish including PR #317-#320.

### Re: nested status:sync, reported-item integration, four new subagents — opus-5.5 · 2026-10-20T10:51-04:00

**Nested `status:sync`: a subagent is fixing the cause.** agent-doc's own commit `dc6186d4da` (10:27:42) wrote a second `<!-- status:sync -->` block inside the first one, and duplicated the "Review pass done…" paragraph into it. The three commits before it had one block. After that, preflight refused every turn (`agent:status contains duplicate or nested status:sync blocks`). The block on disk is a single region again now, but the "Review pass done…" paragraph still sits inside it as well as above it. The subagent is working in its own worktree (`fix/status-sync-nesting`). It is:
- finding every code path that writes `status:sync` / `agent:status`;
- replacing string/regex splicing with a parse of the component into a structure (a single sync region) that is then re-rendered, so a write replaces the region and never wraps it;
- making the writer collapse an already-nested block on its next write;
- adding a regression test with this document's exact shape, plus an idempotence test.

It has been told not to touch this document. I'll report its result when it finishes.

**The five reported heads from the previous session are now closed:**
- **Merged and shipped in v0.35.528:**
  - 1 tmux pane with 2 editor panes: `86a3e164b0`.
  - The background drain typing into a busy turn: `479418fe3c`.
  - Claim timing in the orphan drain: `d9f4aa0031`.
  - GH #320 option A: `d774f7d8bb`. The issue is closed.
- **Integrated now: agent-authored echoes read back as steering.** Commit `3f189816aa` had been pushed to its branch but never merged. I cherry-picked it onto main and moved its `VERSIONS.md` entry under a new `## Unreleased` section above 0.35.528. `make check` passed: 11148/11148 tests, exit 0, no `result: FAILED`. It is pushed to main as `7d05f48b36`, and `make install` exited 0. It is **not released yet**; `agent-doc --version` still reports 0.35.528, but this is a local build of the new main.
- **"release + publish including PR #317-#320" is covered:** #317–#319 shipped in v0.35.527 and #320 in v0.35.528.

**Four heads are dispatched to background subagents, each in its own worktree outside agent-loop:**

| Head | Owner | Branch / worktree |
|---|---|---|
| #gh-fix 321 (stash pane stuck at "still booting") | `subagent:gh321`, gen 1 | `fix/gh321-20261020`, new |
| #gh-fix 322 (focus escalation replaces the wrong column) | `subagent:gh322`, gen 1 | `fix/gh322-20261020`, new |
| GH 320 option B (measured Remote Dev split geometry) | `subagent:gh320b`, gen 2 | `feat/rdgeometry-20261020`, resumes about 800 uncommitted lines from the lapsed attempt |
| Run Agent Doc / Sync Tmux Layout onto the Lazily surface path | `subagent:surfacepath`, gen 2 | `feat/lazilyroute-20261020`, resumes about 700 uncommitted lines from the lapsed attempt |

Each one pushes only its own branch. The GH issues will be closed after integration into main. #322, option B, and the surface path all touch the column-order and surface area, so I'll integrate them one at a time and run one `make install` per batch. Option B still needs live JetBrains Client verification by you after it lands.

**CI:** the latest CI run on agent-doc is still in progress, and the one before it passed. This closeout relies on the local `make check` above.

### Re: Subagent completed: `subagent:gh321` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/321” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=7204837cf8a41dae49d407df7778aa065de845fd3b85b427589bfdc7495461bd -->

### Re: Subagent completed: `subagent:gh322` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/322” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=6ff6178f7b6c7acf043b3841d20a89be8f7a13ef1637f0ba5ffcc8505b5456d5 -->

### Re: Subagent completed: `subagent:gh320b` returned work for queue item “GH 320 follow-up (option B): real Remote Dev split geometry. The 262/263 frontend module publishes the client measured editor-window columns (bump the shared surface snapshot contract) and the backend forwards them into the controller surface Source as editor order; the remembered-order fallback, labelled unknown, stays for classic and one-sided installs. Needs live JetBrains Client verification.” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=d6ea3b75614acbc7c7005237a8bc896db18e15ceb75e9160ed516a41e9eb1168 -->

### Re: Subagent completed: `subagent:surfacepath` returned work for queue item “Move Run Agent Doc and Sync Tmux Layout off the --col CLI shell-out onto the Lazily surface path (specs/13-state-backbone.md "State Propagation, Not Effect Inference").” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=2128d7c5b9d20fcfe4088ee2a070ab6246310be3ce7fd6e18095e3a0bd01ed69 -->
> **Chat prompt (#chatprompt):** I'm curious as to why the tmux panes get into a swapped state and do not self-correct. Is the model data swapped?

### Re: why swapped tmux panes don't self-correct — opus-5.5 · 2026-10-20T11:20-04:00

**Yes. When the panes are swapped, the controller's model is swapped, and tmux is faithfully showing it.** tmux is only a picture of the controller's desired layout (`pane_layout_desired_published columns=[…]`). The reconciler (`tmux_pane_order`) checks tmux against that model and nothing else. When the model has the wrong order, tmux matches the model, the reconciler logs `outcome=held swaps=[]`, and it counts that as converged. Nothing in the loop knows the model itself is wrong. For tmux to correct itself, some input has to bring the editor's real left-to-right order back into the model.

**Three ways the wrong order gets into the model and stays there:**
1. **Remote Dev has no geometry (#320).** The backend only sees a flat list of open editors, not which split is on the left. Since option A (v0.35.528), that order is labelled `column_order=unknown`. The controller does not apply an unknown order as editor geometry. Instead it falls back on the order it already holds, which avoids flapping on non-geometric input. The cost is that moving a document to the other split produces nothing that contradicts the held order, so the swap persists until a manual `agent-doc sync --col … --col …`.
2. **The held order overrides incoming order (#320, #322).** A route marked `column_order=retained` is re-sorted to match the held layout. In #322's log the plugin sent `incoming=[a, b]` and the controller published `columns=[b, a] reordered=true`. That is the model's stale memory winning over the editor by design.
3. **Focus replaced the wrong column (#311/#322).** When focus lands on a document that isn't in tmux, the controller replaces the latest focus's column. With two splits, that is the split you just left, which is still visible. The model ends up with two documents trading one column while the other column is never replaced. tmux reflects it exactly.

**Local IDE windows are fine.** They report `column_order=editor` from measured window positions, so every observation carries the real order and a swap corrects itself. The current ops.log segment here is all local: 64 of 64 publications are `column_order=editor reordered=false`. Right now the model is `[agent-doc-bugs.md, sdk-demo.md]` and tmux `0:0` is `[%2, %58]` in that order, so there is no swap on this machine at the moment. The swapped cases came from the Remote Dev setups in #320 and #322.

**The fixes all aim at the model's input, not at tmux:**
- **Option B (`feat/rdgeometry-20261020`):** the JetBrains Client measures its own editor-window positions and sends them as `column_order=editor`, so Remote Dev gets a real order to correct against.
- **#322 (`fix/gh322-20261020`):** the controller keeps the set of documents the IDE says it shows, and a focus replaces the column holding a document no split shows.
- **Surface path (`feat/lazilyroute-20261020`):** Run Agent Doc and Sync Tmux Layout publish the editor surface instead of `--col` arguments. Every Run then refreshes the model from the editor's own state.

All three branches have reported back and are being integrated serially now, along with #321.
> **Chat prompt (#chatprompt):** subagent: This session crashed and did not restart. Fix agent-doc causes in a subagent

### Re: crashed session with no restart — dispatched — opus-5.5 · 2026-10-20T11:27-04:00

I dispatched this to a background subagent on branch `fix/sessioncrashnorestart`, worktree `~/worktrees/agent-doc-sessioncrash-20261020`, from origin/main `2d5944fb0e`.

**The session in question:** agent-doc session `3f069c68…`, Claude resume `ba846c4c…`, pane `%2`. It is up again under a new supervisor instance (`7916c036…`, pid 1605424, `restart_count=0`), so the crash happened before about 15:25Z, most likely after this document's 14:16Z commit.

**The subagent's brief:**
- Build the crash timeline read-only from `ops.log`, `supervisor-stderr.log`, `state.db`, the Claude transcript tail, `journalctl` and `coredumpctl`.
- Establish what died (the claude child, the supervisor, or both), with what signal or exit code, and why no restart happened.
- Specifically check whether the v0.35.526–528 install/recycle around that time re-exec'd a supervisor that never came back.
- Fix the root cause with deterministic regression tests and spec updates. A crashed child must restart or surface a clear operator-visible state, never silently vanish. If the root cause is outside agent-doc (e.g. the OOM killer), report that with evidence and still fix the missing restart or signal.

**Related:** the `agent:status` nesting I caused last turn already has a fix on origin/main: `2d5944fb0e` "fix(status): parse status:sync as an AST node so writes never nest it". The nested status text may still need a one-time cleanup.
> **Chat prompt (#chatprompt):** can I clear this session keeping the subagent going?

### Re: clearing while the subagent runs — opus-5.5 · 2026-10-20T11:30-04:00

**Yes, it's safe now.** `/clear` does not stop the background subagent: it keeps working in its own worktree and pushes `fix/sessioncrashnorestart` when done. What a clear loses is the report back, which would land in a fresh context with no memory of the dispatch. To cover that, I added backlog item `sessioncrashnorestart` to the queue. It names the branch, the worktree and the integration steps, so whichever coordinator picks it up merges the fix with the next batch.

To lose nothing at all, wait until the pane footer's `← N agents` count drops. Otherwise, clear whenever you like.
> **Chat prompt (#chatprompt):** Can I clear this session?

### Re: dispatch + clear handoff — opus-5.5 · 2026-10-20T11:45-04:00

**Dispatched this cycle:**
- GH #323, the 263 status bar not showing, possibly caused by the nested `status:sync`: `subagent:gh323` on `fix/gh323`, generation 1, started.
- `sessioncrashnorestart`: its subagent was interrupted by this session's restart and has been resumed. Its partial changes are uncommitted in `~/worktrees/agent-doc-sessioncrash-20261020`, and it will also use this session's 15:30–15:40Z restart as evidence.

**Waiting on the next coordinator cycle:**
- Integrate the four reported (📥) items: GH 320 option B (`subagent:gh320b`), the Lazily surface path (`subagent:surfacepath`), GH #321 and GH #322. They are reported but not yet merged.
- Then `release + publish: Include PR #321-#323` once #323 lands. It is unclaimed, so the next cycle picks it up.

**Clearing:** yes, you can clear. Every live item is either claimed by a running or resumed worker, or reported with its branch named in the queue, so a fresh session can integrate from the document. The running workers keep going after a clear, but their completion reports will land in the cleared context. The next coordinator should check `agent-doc queue claims` and each worker's branch rather than wait for a report.
<!-- agent:boundary:1d99bc2a -->
<!-- /agent:exchange -->

<!--
-->

## Queue: Main
<!-- agent:queue id="main" subagents preset="#spec-test-build-install-commit-push" priority go -->
- 📥 GH 320 follow-up (option B): real Remote Dev split geometry. The 262/263 frontend module publishes the client measured editor-window columns (bump the shared surface snapshot contract) and the backend forwards them into the controller surface Source as editor order; the remembered-order fallback, labelled unknown, stays for classic and one-sided installs. Needs live JetBrains Client verification.
- 📥 Move Run Agent Doc and Sync Tmux Layout off the --col CLI shell-out onto the Lazily surface path (specs/13-state-backbone.md "State Propagation, Not Effect Inference").
- 📥 #gh-fix https://github.com/btakita/agent-doc/issues/321
- 📥 #gh-fix https://github.com/btakita/agent-doc/issues/322
- 🚧 do [#sessioncrashnorestart]
- 🚧 #gh-fix https://github.com/btakita/agent-doc/issues/323
- release + publish: Include PR #321-#323
<!-- /agent:queue -->

## Queue: Issues
<!-- agent:queue id=issues loop interval=1h -->
- 🚧 Watch https://github.com/btakita/agent-doc/issues for new issues. Add new issues into the backlog + queue except issue #40 + #45.
<!-- /agent:queue -->

## Todo
<!-- agent:todo -->
- [ ] [#ahfe] Live-check GH #267 on the 263 IDE: after `agent-doc upgrade` WITHOUT restart, once idea.log shows `[native] hot-reloaded libagent_doc`, Run Agent Doc should complete (ops.log `editor_route_superseded_editor_admitted`, route output ends with the advisory line). 2026-10-15 operator: did not work (believed); watch for bug reports before reopening GH #267.
- [ ] [#ctg7] Under heavy load, switch the editor 2 panes -> 1 -> 2 and confirm tmux follows and stays at 2 (stale-controller fix).
- [ ] [#n7kq] In the sdk.md session: restore the 7 todo ids the 23:30 --todo-edit dropped (#tdclireview, #tdcontracts162, #tdpromotioncallers, #tdinspectclones, #tdfpe202, #tdadr0033, #tdadr0030) with --todo-edit "<exact text>=#tdX <same text>", or accept the ids the preflight backfill assigns
- [ ] [#t1v0] In the sdk.md pane, press Clear Session Context again: the 06:03 presses were held behind the open cycle and then dropped by the install restart. Check first that sdk.md's queue has not opened its tdue4cpuset cycle.
<!-- /agent:todo -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] 🚧 [#sessioncrashnorestart] [agent-doc] Integrate the crashed-session-no-restart fix: a background subagent (dispatched 2026-10-20 ~15:27Z from pane %2) is diagnosing why the agent-doc-bugs.md Claude session (agent-doc session 3f069c68, Claude resume ba846c4c) crashed some time between ~14:16Z and ~15:25Z and the supervisor did not restart it. It pushes branch fix/sessioncrashnorestart (worktree ~/worktrees/agent-doc-sessioncrash-20261020, based on origin/main 2d5944fb0e). When that branch has a commit: review the diagnosis in the commit and spec changes, merge it with the next batch, run make check plus make install, and close this item. If the branch has no commit and the worktree is idle, re-dispatch the investigation.
- [/] [#gh299-live-263-proof] Live-verify v0.35.519 on JetBrains 263.7031 with changed plugin sources: `agent-doc upgrade` must hot-reload and complete without an IDE restart; close GH #299 only with that proof.
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
