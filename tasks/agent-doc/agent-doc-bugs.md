---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: ba846c4c-2e01-4e12-9539-096faec207ee
  codex: 01a12175-b9c0-7db0-af02-218c5501efe7
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

Released agent-doc v0.35.475. v0.35.476 is not yet tagged or published; the actionable review added the release gap and two correctness follow-ups.
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append subagents -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261010-172930.md`*

Compacted content:
- Archived 48 response topic(s): actionable review; queue subagent dispatch (claude coordinator takeover); actionable-review dispatch; 45 more
- Prior summary/context: prior compacted content: Archived 31 response topic(s): retained closeouts, queue integrity, GH #181–#184, and v0.35....

### Re: queue go — two subagent heads dispatched — opus-5-5 · 2026-10-10T13:35-04:00

Both new queue heads carry subagent intent under `#spec-test-build-install-commit-push`, so each was claimed and handed to its own background subagent in a separate agent-doc worktree outside the IDE-watched tree, based on `origin/main` (`ca98f7926`). The local `src/agent-doc` checkout is 356 commits behind, so neither worktree branches from it.

| Item | Claim owner | Worktree / branch |
|---|---|---|
| `#runctrlclaude`: route wedged on an `open_empty_preflight` closeout after ctrl+c and `/clear` | `subagent:runctrlclaude` | `~/work/btakita/agent-doc-wt/runctrlclaude` · `fix/runctrlclaude` |
| Background supervisor thrashing, plus formal cleanup invariants for old supervisors | `subagent:supthrash` | `~/work/btakita/agent-doc-wt/supthrash` · `fix/supthrash` |

Briefs:
- **`#runctrlclaude`:** when the owning harness run is provably gone (for example interrupted, then `/clear`ed), the route should abandon an empty, replay-safe preflight closeout automatically. It should stay fail-closed only while that run may still be live. The subagent will add a regression test and update the spec.
- **Thrash:** the subagent starts from live process and log evidence (CPU use, superseded binaries, processes orphaned to PID 1, controllers stuck in `preparing`). It writes the supervisor lifecycle down as a state machine with explicit invariants: one live supervisor per (document, root), superseded or ownerless supervisors terminate within a bounded time, every spawn has a reaper, and no busy loop runs without progress. Those invariants go into the spec and into tests, and the code paths that break them get fixed. The subagent reports any orphans for the operator to kill instead of killing live supervisors itself.

Subagents commit and push only their own branches. Neither runs `make install`. When they report back, this coordinator integrates both, runs one `make install`, releases the claims, and closes the items.

No new backlog follow-ups this cycle. Both items are already tracked: one by its `#runctrlclaude` backlog entry and both by active queue claims. Any remainder a subagent reports gets captured when it is integrated.

### Re: `#runctrlclaude` re-trigger — already in flight — opus-5-5 · 2026-10-10T13:37-04:00

`#runctrlclaude` is already claimed by `subagent:runctrlclaude`, which is working in `~/work/btakita/agent-doc-wt/runctrlclaude` on `fix/runctrlclaude`. This cycle does no inline work on it, to avoid two agents touching the same fix. The item stays open until the subagent reports back. Then the coordinator merges the branch, runs one `make install`, releases the claim, and records `--done`. The thrash item is also still in flight, under `subagent:supthrash`.

This re-trigger produced no new follow-ups.

### Re: `#runctrlclaude` pressed again — still in flight — opus-5-5 · 2026-10-10T13:39-04:00

According to ops.log, this cycle came from an explicit Run Agent Doc (`explicit_route=true`), not the supervisor. The head still belongs to `subagent:runctrlclaude`, and its claim runs until 1791660752, about two hours out. That subagent is in the middle of the fix on `fix/runctrlclaude`, so this cycle does no inline work on it. The item closes after the coordinator merges that branch and runs `make install`.

Each press still opens a full no-op cycle, even though zero heads are drainable. I've filed that as a separate backlog follow-up: route/preflight should report which subagent owns the claimed head and when the claim expires, without opening a cycle.

### Re: batch status — release in flight, three new subagents — opus-5-5 · 2026-10-10T14:20-04:00

**Integration and release**
- **`#runctrlclaude`:** fixed (`b555f6f0e`) and fast-forwarded onto agent-doc `main`.
  - **Root cause:** Ctrl-C landed during the preflight hook, so no interrupt line or Stop hook marked the turn as ended. `/clear` only cleared the running-turn flag, which proves no turn is known to be running but not that the cycle's owner is gone.
  - **Fix:** a durable per-pane turn-end receipt. The route and the idle supervisor now drop an empty preflight right away when the pane is idle and the receipt is newer than the cycle. Otherwise they stay fail-closed.
  - **Not covered:** the Clear Session Context guard itself, which blocked before your manual `/clear`.
- **Release 0.35.484:** VERSIONS.md entry and version projection (`db6c9d699`), plus an editor-parity row (`cebc7972b`, the first release attempt failed `release-preflight` without it). `make release` is running now: `make check` and `tmux-ci`, then tag, publish and `install-full`.
- **Supervisor thrash:** fixed on `fix/supthrash` (`65e71d9f5`). It merges after the release finishes and goes out with the next install.
  - **Cause 1:** a self-handoff that could never succeed was retried every ~5s. Its socket path was 112–123 bytes, over the 107-byte limit: 104k failures on `editors/jetbrains`, and 6.8k spawned replacements on `monsterrodholders-dev`.
  - **Cause 2:** idle-root controllers never retired. Closed actor rows counted as ownership, and current-binary controllers had no idle exit, so `$HOME` reached generation 175.
  - **Invariants L1–L5:** at most one controller per root; an idle root's controller exits within a bounded time; a controller never exits while its root is in use; every retry loop is bounded; the handoff socket fits whenever `controller.sock` fits. They're written into `specs/08b` and model-checked in `formal/tla/ControllerLifecycle.tla`, where 3 wedge configs must fail and do.
  - **Tests:** 9914/9914 nextest passed, `make tla` green.
  - **Orphans the subagent recommends you SIGTERM:** `1704535 2038791`, which are old-binary handoff loopers that the install can't fix. Optionally also the idle-root controllers `3151025 3151027 3151028 3151032 3151046`. Leave 3169918 and 3170850 alone; they serve live sessions.
  - **Still open:** a supervisor on a superseded binary that waits forever for a turn boundary (betterstack pid 3806401), and the cost of the ~17s `midturn_steering` polling.

**Dispatched this cycle**, each claimed and in its own worktree off `cebc7972b`:

| Item | Owner | Brief |
|---|---|---|
| `#runfrontenderror` | `subagent:runfrontenderror` | First decide whether this is `#runfrontenddispatch` coming back or a new path (frontend.md supervisor pid 1989646 runs stale 0.35.481). Then make the dispatch refusal time-bounded instead of indefinite. |
| `#explicitrunqueue` | `subagent:explicitrunqueue` | An explicit Run Agent Doc where every changed head is claimed reports `claimed by <owner>, expires <t>` and opens no cycle. |
| Multiline queue cruft | `subagent:queuecruft` | A multi-line free-text queue item is treated as one item when it's promoted to the backlog or reordered. The orphaned "route-owned host supervisor (pid 1989646)…" line gets cleaned up once the fix lands. |

**New defect captured:** the supervisor-thrash queue line was auto-struck as "tracked by backlog #runctrlclaude" only because that backlog item's text quotes it. I added this to the backlog as its own item. The thrash work itself is unaffected, still under its own claim.

`#runctrlclaude` and the thrash item close once the release has installed and the thrash branch is integrated and installed.

### Re: two new ids — one re-claimed, one dispatched — opus-5-5 · 2026-10-10T14:30-04:00

- **`#runfrontendcrashed`** is the frontend.md crash item, including the `img_3.png` terminal-formatting screenshot, after promotion to the backlog. When the free-text head became an id, its existing claim did not move with it, so it showed up again as an unclaimed dispatch. I re-claimed it under the subagent already working on it, `subagent:frontendcrash`, so no second subagent runs. The fix for the claim not following a promoted item is folded into `#freetextqueue`.
- **`#freetextqueue`** is dispatched to `subagent:freetextqueue` (worktree `agent-doc-wt/freetextqueue`, based on `cebc7972b`). Its brief: a free-text head counts as tracked by a backlog item only through exact promotion lineage, never because its text appears inside unrelated prose. A claim also moves with an item when it is promoted.
- The orphaned "route-owned host supervisor (pid 1989646)…" line is gone from the queue. `subagent:queuecruft` is still fixing the cause.

**Release 0.35.484:** full nextest passed (9921/9921). tmux-ci, tag, publish and `install-full` are still running.

**Still in flight:** `#runfrontenderror`, `#explicitrunqueue`, queue cruft, `#runfrontendcrashed`, `#freetextqueue`. The supervisor-thrash fix (`fix/supthrash`) merges after the release.

This cycle had no new follow-ups: the claim-transfer defect is covered by `#freetextqueue`.
> **Chat prompt (#chatprompt):** Instead of rerunning flaky tests, fix the cause of the flakyness

> **Chat prompt (#chatprompt):** cont

### Re: fix flaky causes, don't rerun — opus-5-5 · 2026-10-10T14:58-04:00

I sent this directive to four of the five running subagents: `#runfrontenderror`, `#explicitrunqueue`, queue cruft and `#freetextqueue`. You rejected the send to the `#runfrontendcrashed` subagent, so that one has not received it. For every test that failed in any of its `make check` runs, even one that later passed on a rerun, each subagent has to name the test, show the failure, and find the cause. Then it makes the test deterministic: an injected or simulated clock, waiting for the actual event instead of a timer, and unique per-test paths. A longer timeout doesn't count as a fix. The flake fix goes in its own commit, and the final report lists each flaky test with its cause.

**Likely trigger:** the machine's load average is about 128, from 5 full `make check` suites plus the 0.35.484 release running at once. That load exposes tests that depend on wall-clock timing. The load explains when the tests fail, but the tests still have to be fixed so load can't make them fail.

**Release 0.35.484:** still running, with no failures or retries in its log so far.

### Re: 0.35.484 released; `#runctrlclaude` closed — opus-5-5 · 2026-10-10T15:35-04:00

> **Queue prompt:** release + publish

**v0.35.484 is released and installed.** Tag `v0.35.484` is pushed, `make release` exited 0 with `tag_publish_handoff=complete` and `local_install_full=complete`, and `agent-doc --version` reports 0.35.484. The release ran 9921/9921 nextest plus tmux-ci on the version-projected tree. It contains the `#runctrlclaude` fix (`b555f6f0e`), so `#runctrlclaude` is done.

Two steps were needed to get there:
- The first `make release` failed in `release-preflight` because `editors/release-parity.tsv` had no row for 0.35.484. I added the row (`cebc7972b`).
- This cycle re-surfaced `#runctrlclaude` as an unclaimed subagent dispatch because its 2-hour claim TTL had expired. Closing it here resolves that, and I refreshed every live claim so no other in-flight item re-dispatches.

**The supervisor-thrash fix is held from `main`.** I rebased it onto 0.35.484 (`b7b472719`), resolving only a version conflict in `agent-doc-gc-io/Cargo.toml`. Its `make check` then failed one test:
- **Failing test:** `codex_hook_integration::codex_hook_cli_replays_plain_final_answer_after_repeated_auto_queue_stop`.
- **What happened:** the Stop hook hit its 45s budget, with `auto_queue_continuation` running for 42.5s at load average ~100–128. About 30s of that was a silent wait.
- **Next:** per your directive I didn't rerun it. `subagent:codexstopflake` (worktree `agent-doc-wt/codexstopflake`) is finding the wait, deciding whether supthrash caused it, and fixing the cause with load-stress proof. Once that lands, supthrash integrates with one `make install`.

**Test-isolation hazard, filed in the backlog.** Subagents had pointed TMPDIR under `$HOME`. agent-doc tests then walked up to `~/.agent-doc` and wrote your real `state.db`, `ops.log` and snapshots, and started controllers rooted at `/home/brian`.
- That caused some of the "flaky" failures, such as `database is locked`.
- All subagents now use `/tmp` paths and stop only their own leftover processes.
- No test controller rooted at `$HOME` is still running.
- The backlog item asks for a project-root ceiling under test, plus a harness guard against a TMPDIR with an `.agent-doc` ancestor. `~/.agent-doc` is not cleaned yet; that waits for your OK.

**Still in flight:** `#runfrontenderror`, `#explicitrunqueue`, `#runfrontendcrashed`, `#freetextqueue`, the multi-line queue cruft, and the codex Stop-hook flake.
<!-- agent:boundary:a19d933a -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built
<!-- agent:queue subagents preset="#spec-test-build-install-commit-push" priority go -->
- 🚧 do [#runfrontendcrashed]
- ⏭️ do [#runfrontenderror]
- ⏭️ do [#runctrlclaude]
- 🚧 do [#explicitrunqueue]
- 🚧 do [#freetextqueue]
- 🚧 Multiline cruft was left from the `Run Agent Doc on frontend.md error` free-text queue item that transitioned into the backlog.
- ~~release + publish~~ — auto-struck: answered this cycle (#ftstrike)
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#testisolationtests] Test isolation: agent-doc tests whose TMPDIR sits under $HOME walk up past their tempdir, adopt /home/brian/.agent-doc as the project root, and write the operator's real ~/.agent-doc/state.db, ops.log, proof-ledger and snapshots, and spawn controllers with --project-root /home/brian (seen 2026-10-10: 'failed to save registry to /home/brian/.agent-doc/state.db: database is locked', a leaked test controller, and subagent scratch paths in ~/.agent-doc/logs/ops.log). Project-root discovery under test must stop at a ceiling (the test tempdir / an AGENT_DOC_ROOT_CEILING like GIT_CEILING_DIRECTORIES), and the test harness must refuse a TMPDIR that has an .agent-doc ancestor. Then scrub the test-written entries from ~/.agent-doc with the operator's OK.
- [ ] 🚧 [#runfrontendcrashed] Run Agent Doc on frontend.md crashed my Claude Code session and unsuccessfully restarted. The raw prompt was the end state. The terminal formatting is also messed up ![img_3.png](img_3.png).
- [ ] 🚧 [#freetextqueue] Free-text queue head wrongly auto-struck by #qftbklgstrike: the supervisor-thrash queue line was struck as 'tracked by backlog #runctrlclaude' only because #runctrlclaude's backlog text QUOTES that line (inside the route error message), even though the two are different defects and the thrash fix was still in flight under its own claim (subagent:supthrash). Containment of a head's text inside an unrelated backlog item's prose must not count as tracking; require an exact promotion lineage/receipt. 2026-10-10 ~17:45Z, agent-doc-bugs.md.
- [ ] 🚧 [#runfrontenderror] Run Agent Doc on frontend.md error: dispatch-only claude reopen refused to inject into pane %166 for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/frontend.md because the project supervisor is mid-recycle (reason=auto_install_reexec); a trigger typed across the hot-reload boundary would be dropped before submit. Retry once the supervisor settles onto the fresh binary ui_outcome_contract=ui-outcome-v1 ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=follow_unblocker unblocker=wait_for_supervisor_recycle_settle
- [ ] 🚧 [#explicitrunqueue] Explicit Run Agent Doc on a queue head that is claimed by a live subagent (agent-doc queue claims shows an unexpired owner) still admits a full cycle with queue_drainable_head_count=0, so each press commits a no-op in-flight response and trips expect_done_or_gate_guard. The route/preflight should answer 'claimed by <owner>, expires <t>' without opening a cycle. Repro: 2026-10-10 17:34Z, ops.log preflight_turn_admission explicit_route=true for the claimed #runctrlclaude.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#operatorverifylive] #frontendlivedispatch [operator-verify] [agent-doc] Live confirmation that Run Agent Doc on haiven-dev frontend.md dispatches again. The fixes shipped in 0.35.483: runfrontenddispatch 3d9fbad86 and dispatchreadyselfheal 81b9e1910. The existing wedge (started 4351 / settled 4350) is still in place because pane %166's supervisor (pid 1989646, started about 15h ago on the pre-0.35.483 binary, last heartbeat 03:49Z) has not re-registered, so its lease has no ready_registered_at stamp, and install_fanout recycle requests are ignored while the old recycle is in flight. Unblocker: restart the frontend.md session in pane %166, or let that supervisor re-register. Then click Run Agent Doc and look for route_dispatch_only_recycle_inflight_ready_after_start outcome=settled, or supervisor_recycle_graph_settled reason=dispatch_ready_after_start, in haiven-dev ops.log, with no refusal=recycle_ttl_elapsed_supervisor_alive.
- [/] [#frontendlivedispatch] [operator-verify] [agent-doc] Live confirmation that Run Agent Doc on haiven-dev frontend.md dispatches again. The fixes shipped in 0.35.483: runfrontenddispatch 3d9fbad86 and dispatchreadyselfheal 81b9e1910. The existing wedge (started 4351 / settled 4350) is still in place because pane %166's supervisor (pid 1989646, started about 15h ago on the pre-0.35.483 binary, last heartbeat 03:49Z) has not re-registered, so its lease has no ready_registered_at stamp, and install_fanout recycle requests are ignored while the old recycle is in flight. Unblocker: restart the frontend.md session in pane %166, or let that supervisor re-register. Then click Run Agent Doc and look for route_dispatch_only_recycle_inflight_ready_after_start outcome=settled, or supervisor_recycle_graph_settled reason=dispatch_ready_after_start, in haiven-dev ops.log, with no refusal=recycle_ttl_elapsed_supervisor_alive.
- [/] [#jb262dynlauncherrecommen] [recommended] [operator-verify] [agent-doc] Operator decision: jb262dynupgrade (153c72916) chose the accurate-warning option (a 262 package reports declined_by=modular_package and falls back to file replacement + restart). Either keep that as the supported policy (then close this), or authorize a real restart-free launcher for the exact-262 split-mode plugin that swaps backend + frontend (the frontend may run in a separate JetBrains Client process), proven against a live 262 IDE.
- [/] [#jetbrains262distribution] [agent-doc] Provide and verify a two-sided production distribution channel for the exact-262 modular plugin before calling v0.35.480 generally installable: publish classic and 262 as non-overlapping updates of the same Marketplace plugin ID, or ship an equivalent tested custom-repository feed, while preserving fail-closed behavior for one-sided installation. Verify clean 262 Remote Dev installation reaches backend and Client/Gateway, 242–261 receive only classic, mismatch remains fail closed, and the release workflow asserts both ranged updates.
- [/] [#gh218remoteproof] [operator-verify] GH 218: PR 222 is MERGED (01:25Z 2026-10-10, merge cc6e458c6) at head 34842a23a, CI run 38002449411 green. Run the real IDEA 2026.2.3 (262) Remote Dev acceptance matrix against prerelease jetbrains-262-preview-34842a2 (asset agent-doc-jetbrains-262-0.2.511.zip, id 626772959, 2556136 bytes, sha256 2ef328588d1491446f1a45a83775ee220c13507f5cdd9bca4f5c2111f9e864a7), or against the 0.2.514 ZIP of the next published release. Verify both downloaded copies with make verify-jetbrains-262-preview BACKEND_ZIP=... CLIENT_GATEWAY_ZIP=... first, and install the same ZIP on backend and Client/Gateway. Check: detached-only isolated terminal; same-document main-wins read-only placeholder with byte-identical main; focus/sync/resize causes no main mutation; close/rejoin; backend restart/client reconnect before the first main publication; two simultaneous thin clients; capability loss/one-sided install fails closed. Post results on 218 citing that tag and commit, then close 218. jetbrains-262-preview-c39de1b is superseded.
- [/] [#drivecapturedsplice] [operator-verify] Captured-splice stale-replica live proof (plan: tasks/agent-doc/plan-captured-editor-splice-recovery.md). Recipe and regression test shipped in v0.35.466; live proof remains gated because the corrected run reached owner pane %134 but stopped before the retained-delivery marker, edit two, and verifier completion. Rerun the recipe through verify-captured-splice-recovery and require it to pass.
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically by a human reply on pypi/support#12214, or make pypi-quota-check reporting a ceiling in reach. Rechecked 2026-10-06: 8.363 GiB of 10.0 GiB, 1.637 GiB headroom, 198 releases through 0.35.465, post-cutover mean 35.3 KiB, approximately 48,621 further releases fit; VERDICT headroom fine. The support issue remains open with only the github-actions bot comment and has not changed since 2026-09-10. Nothing to act on or delete. If the grant lands, record it and drop this item; deletion stays reserved for a ceiling the authenticated check reports.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#recommendedtwojetbrains] #ideriderstopgap [recommended] [agent-doc] Two JetBrains IDEs on one project: only the most recently focused IDE drives the shared tmux agent-doc window, which stops the layout from flipping between IDEA and Rider. About 1 day. Builds on idearidereditors 9796d8c8a (installation-scoped editor-surface client families).
- [ ] [#recommendedfullyindepend] #ideriderindependentnav [recommended] [agent-doc] Fully independent navigation per JetBrains installation: one tmux window or session per IDE installation. The installation identity becomes part of the layout-ownership key, tmux-to-editor focus mirroring goes only to the IDE that owns the window, tmux_focus_state and pane ownership are split per window, and specs plus a two-IDE SimWorld are added. Several days.
- [ ] [#recommendedwriteruntime] #visiblewriteloadtimeout [recommended] [agent-doc] agent-doc-write-runtime-io visible_write_content_snapshot_tests (test_lazily_visible_write_event_read, test_poll_lazily_event_*) fail under host load (load average around 50) with 'timed out after 2.0s waiting for project controller response' at ipc.rs:170 (record_lazily_visible_write_candidate). Seen in 2 of about 6 full runs on 2026-10-10. Make the fixture warm the controller before the timed RPC, or charge cold-start launch separately from CONTROLLER_RPC_TIMEOUT, so a busy runner cannot decide the outcome.
- [ ] [#ideriderstopgap] [recommended] [agent-doc] Two JetBrains IDEs on one project: only the most recently focused IDE drives the shared tmux agent-doc window, which stops the layout from flipping between IDEA and Rider. About 1 day. Builds on idearidereditors 9796d8c8a (installation-scoped editor-surface client families).
- [ ] [#ideriderindependentnav] [recommended] [agent-doc] Fully independent navigation per JetBrains installation: one tmux window or session per IDE installation. The installation identity becomes part of the layout-ownership key, tmux-to-editor focus mirroring goes only to the IDE that owns the window, tmux_focus_state and pane ownership are split per window, and specs plus a two-IDE SimWorld are added. Several days.
- [ ] [#visiblewriteloadtimeout] [recommended] [agent-doc] agent-doc-write-runtime-io visible_write_content_snapshot_tests (test_lazily_visible_write_event_read, test_poll_lazily_event_*) fail under host load (load average around 50) with 'timed out after 2.0s waiting for project controller response' at ipc.rs:170 (record_lazily_visible_write_candidate). Seen in 2 of about 6 full runs on 2026-10-10. Make the fixture warm the controller before the timed RPC, or charge cold-start launch separately from CONTROLLER_RPC_TIMEOUT, so a busy runner cannot decide the outcome.
- [ ] [#ghrecvpolicy] [agent-doc] Recipient-side [receive] auto_create_components policy for multi-user workspaces (GH #40, https://github.com/btakita/agent-doc/issues/40). Parked until multi-user workspace support exists.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
