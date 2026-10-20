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

📥 Queue handoff: 9 reported items awaiting coordinator integration, verification, and closeout.
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

### Re: Subagent completed: `subagent:gh323` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/323” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=8609f9b8859d899d1eb9b71d5fc105c90fbaf8218325fdc7f736eb33a03d5a78 -->

### Re: Subagent completed: `subagent:sessioncrashnorestart` returned work for queue item “do [#sessioncrashnorestart]” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=1685e25a07197fe126e3ea09ca248cdbe87c016b006d74eb0f5e0af7c2ba330a -->
> **Chat prompt (#chatprompt):** Can I clear this session?

### Re: v0.35.529 release handoff — opus-5.5 · 2026-10-20T12:20-04:00

**Clearing is safe once this handoff is committed.** The in-flight v0.35.529 release is coordinator-owned (`coordinator:release-0529`, generation 1, worker `coordinator-ba846c4c`) and is recorded here, so a fresh session can finish it.

**State at handoff:**
- **Integration worktree:** `~/worktrees/agent-doc-batch0529-20261020`, branch `integrate/batch0529`, HEAD `2c210f307` `release: v0.35.529`, which fast-forwards from origin/main `2d5944fb0e`.
- **Merged:** `fix/gh321-20261020` (GH #321), `fix/gh322-20261020` (GH #322), `feat/lazilyroute-20261020` (surface path), `feat/rdgeometry-20261020` (GH 320 option B), `fix/gh323` (GH #323), and `fix/sessioncrashnorestart`.
- **Conflicts resolved:**
  - `VERSIONS.md`: kept both entries.
  - `SPEC.md` and `editors/jetbrains/SPEC.md`: took the option B Remote Dev text, then appended the surface-reference bullet.
  - Plugin generations re-bumped with `scripts/check_plugin_versions.py --bump`: classic 0.2.729, 262 0.2.730, 263 0.2.731.
- **Release metadata:** VERSIONS.md now has a 0.35.529 entry, with GH #321/#322/#323 notes added, plus a release-parity row.
- **Check:** the authoritative `make check` is running in the background. Its log is `~/worktrees/agent-doc-batch0529-20261020/make-check-0529.log`, which ends with `EXIT=<code>`.

**To finish:**
1. Confirm `EXIT=0` and no `result: FAILED` in that log, then delete the log (it is untracked).
2. `git push origin HEAD:main`, fast-forward only.
3. `make release` in that worktree, and verify the GitHub Release v0.35.529 assets, PyPI 0.35.529, and the installed `agent-doc --version`.
4. Close the shipped heads:
   - `--done sessioncrashnorestart`;
   - echo-strike the free-text heads for GH #321, #322 and #323, the GH 320 option B follow-up, the surface-path move, and this `release + publish` head;
   - close GH #321 and #322 on GitHub if they are still open (#323 is already closed).

If the check fails, fix the cause in the integration worktree; never rerun it to green.

### Re: v0.35.529 release blocked on a flaky tmux test — opus-5.5 · 2026-10-20T13:05-04:00

**Not released yet.** The authoritative `make check` on the release tree passed: 11192/11192 nextest, exit 0. The release commit `2c210f307` `release: v0.35.529` is fast-forwarded onto `main`. But `make release` failed in its tmux-ci step (exit 2), so no tag was pushed and nothing was published. PyPI is still at 0.35.528.

**The failure:** `sync::tests::find_live_owner_pane_prefers_latest_open_session_log_owner_over_stale_process_tree_match` panicked on its own fixture precondition (`sync.rs:8340`: "stale pane should expose a same-file process-tree match before session-log precedence is evaluated"). Run alone, it passes in 2.15s, so it depends on timing: it failed in the serial tmux-ci run while another subagent's `make check-fast` loaded the machine.

**Per the fix-the-cause rule:** I didn't rerun it to green. A subagent on `fix/ownerpaneflake` (from `2c210f307`) is making the fixture wait for the observable process-tree event instead of a deadline, or fixing the production race if that is the real cause. It must prove the fix under CPU load. Then I re-run `make check` and `make release` as v0.35.529, since the tag is still unused. The release claim (`coordinator:release-0529`) is refreshed.

**Also running:** `subagent:clauderesume` on `clauderesumeafterclear`.

### Re: GH #324–#327 dispatched; v0.35.529 release still publishing

Each new issue has its own subagent in its own worktree (`~/worktrees/agent-doc-gh<N>-20261020`, branch `fix/gh<N>`, cut from main `181bd291b2`):

| Issue | What it fixes | Queue state |
|---|---|---|
| #324 | A focused extract removed linked exchange turns from the source, but the target never received them | 🚧 running |
| #325 | Preflight resolves `agent:done archive=` as a path under the project root, so a bare `archive`/`document:` anchor never resolves | 🚧 running |
| #326 | JetBrains 263: move frequently changed plugin logic behind the hot-reloaded native library so upgrades don't need an IDE restart | 🚧 running |
| #327 | A `loop` queue loses its item on every pass | 🚧 running |

For #326, the subagent was asked to ship one complete slice and to report the remaining phases as follow-up queue heads, rather than leave a half-done refactor.

After all four report, I'll merge their branches, then run one `make check` and one `make install`. The release you queued for #324–#327 comes after that.

The v0.35.529 `make release` is past tagging and is waiting on the GitHub Release workflow. Its closeout is still pending:
- confirm the GitHub Release assets and PyPI;
- strike the shipped heads (GH #321/#322/#323, `#sessioncrashnorestart`, `#clauderesumeafterclear`);
- close GH #321 and #322 on GitHub;
- file the crash-auto-restart follow-up;
- repair `agent:status`.

### Re: Subagent completed: `subagent:gh325` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/325” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=e3beb2fe6c10ea183d27df00f695ea40aea21bb1aee481eb6024dd972e338276 -->

### Re: Subagent completed: `subagent:gh324` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/324” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=eb7f94461977ea76e333e08a784202869dc145af6bd00e9915fcf7337849a452 -->
> **Chat prompt (#chatprompt):** subagent: fix • Blocked by hook
>   └ Queue continuation, not an error: Codex labels the Stop continuation wire shape `Blocked by hook`. Next
> `agent:queue auto` prompt for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-dev.md:
> "[#tdue4decisions]: Are you suggesting that we transition the entire sdk to use float or only UE 6?". Continue THIS
> turn in-pane: answer it, then persist with `agent-doc finalize /home/brian/work/btakita/agent-loop/src/haiven-dev/
> tasks/sdk/sdk-dev.md`. Do NOT run `agent-doc /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-dev.md`
> (recursive self-invocation) and do not send the final answer yet.

> **Chat prompt (#chatprompt):** unless this is normal behavior

### Re: Codex "Blocked by hook" on sdk-dev.md

**The label is normal.** Codex can only continue a turn from its Stop hook with `decision: "block"`, and Codex shows that as `Blocked by hook`. That's why the payload starts with "Queue continuation, not an error". This is documented in `SPEC.md` and `runbooks/harness-invocation.md` (`#codex-stop-continuation-label`), and the label won't change.

**What isn't normal is that it continued in the pane at all.** sdk-dev.md's queue has auto-clear on by default. With a healthy supervisor, the Stop hook should have let Codex stop at the item boundary, and the supervisor would then restart Codex fresh for the next item (`#bgclearpolicy`). Instead, `ops.log` shows:
- **17:30:11Z**: the install recycle skipped pane `%22`'s supervisor as `cause=no_longer_live`. Pane `%53`, running sdk-dev-loop.md, was skipped the same way.
- **17:51:16Z**: the Stop hook logged `codex_stop_queue_auto_clear_kept_in_pane supervisor_drain_readiness=supervisor_heartbeat_stale`.

The supervisor was alive the whole time: PID 1728757, a child of pane `%22`, running the current 0.35.529 binary and re-executed in place. agent-doc's liveness and heartbeat checks reported a live supervisor as stale, which is the bug.

I've dispatched a subagent to find the root cause in code, on branch `fix/codexstoplabel` in `~/worktrees/agent-doc-codexstoplabel-20261020`. It will leave the live supervisor and the haiven-dev documents untouched. If the evidence shows this is designed behaviour after all, it will report back without changing code.

The sdk-dev Codex turn itself is fine: it should answer `#tdue4decisions` in its pane and finalize as normal.

**Other work in flight:**
- GH #324 and #325: reported; waiting to be integrated.
- GH #326 and #327: still running.
- v0.35.529: still waiting on the GitHub Release workflow.

### Re: Subagent completed: `subagent:gh327` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/327” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=c5b5a269fbbeaabc1d5b161cd3807c26a0889cdf091af760fc0c0adcb0d62f0e -->

### Re: Subagent completed: `subagent:gh326` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/326” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=77b361abfd695727759a55d7e318582ce8aba430623ffbee8fa6f5f404ab2233 -->
> **Chat prompt (#chatprompt):** Will this release include fixes for PR #324-#327?

### Re: Which release carries #324–#327

**No, not the one publishing now.** v0.35.529 was tagged from `181bd291b2` before the #324–#327 subagents started, so it ships only #321–#323, the session-crash fixes, `#clauderesumeafterclear` and `#ownerpaneflake`. Its GitHub Release workflow is still waiting on the Intel macOS build.

**#324–#327 will ship as v0.35.530**, the "release + publish: Include PR #324-#327" item you queued. All four are merged into `integrate/batch0530`, and `make check` is running on it. One thing gates that release: the main CI test `queue_auto_clear_types_clear_between_items_into_the_same_harness` failed on GitHub's runner, and `make release` runs the same tmux suite. The supervisor-heartbeat subagent is checking whether it's the same bug. I'll release v0.35.530 once that fix is merged too, or once it's shown to be separate and fixed on its own.

Note on #326: v0.35.530 carries only its first part, the layout rules moved into the hot-reloaded library. Four more parts follow as queue items.

### Re: Your edit "release + publish: Include PR #321-#327"; heartbeat fix done; CI flake dispatched

**Release item.** v0.35.529 was already tagged from `181bd291b2` and is publishing; a published tag can't take more commits. So I'm keeping your edited item 🚧 and will close it with **v0.35.530**, which adds #324–#327 on top of 0.35.529's #321–#323. The separate "release + publish: Include PR #324-#327" item closes with it.

**Supervisor heartbeat (sdk-dev pane `%22`) fixed** on `fix/codexstoplabel` (`08dad88263`), reported 📥.
- **The bug:** a supervisor writes its heartbeat only on state changes, never while a turn runs. Any Codex turn longer than 60s therefore read as "heartbeat stale", the Stop hook kept the queue in the pane, and the fresh-restart path never ran. The sdk-dev turn's heartbeat was 503s old.
- **The fix:** a live supervisor on the current binary whose `agent-doc start` command line names the document now counts as ready, whatever its heartbeat age. A supervisor that never drains is still caught by the existing 300s overdue check. Preflight, session-check and the Stop hook all use this check.
- **Tests:** 3 new regression tests replay the incident's real timestamps, and they fail with the fix disabled. `make check-fast` passed.
- **The 17:30 `no_longer_live` line is normal:** the supervisor had already re-exec'd onto the new binary, so the second install request had nothing left to do.

**The CI failure has a different cause.** The supervisor's own auto-clear doesn't consult the heartbeat. The likely cause is a startup race: on a slow runner, the supervisor records its "before" point after item one has already committed, so item one never counts as finished and no `/clear` is typed. I've dispatched a subagent to reproduce this deterministically and fix it in the product, not by lengthening timeouts, on `fix/autoclearbaseline` (🚧). v0.35.530 waits for it, because `make release` runs the same tmux suite.

### Re: v0.35.529 published; closeout done; five follow-ups dispatched

> **Queue prompt:** GH 320 follow-up (option B): real Remote Dev split geometry. The 262/263 frontend module publishes the client measured editor-window columns (bump the shared surface snapshot contract) and the backend forwards them into the controller surface Source as editor order; the remembered-order fallback, labelled unknown, stays for classic and one-sided installs. Needs live JetBrains Client verification.
> **Queue prompt:** Move Run Agent Doc and Sync Tmux Layout off the --col CLI shell-out onto the Lazily surface path (specs/13-state-backbone.md "State Propagation, Not Effect Inference").
> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/321
> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/322
> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/323
> **Queue prompt:** release + publish: Include PR #321-#323

**v0.35.529 is published and verified:**
- **GitHub Release:** 15 assets, published 18:18:04 UTC.
- **PyPI:** 0.35.529 published 18:18:53 UTC.
- **Local install:** `agent-doc --version` is 0.35.529.
- **JetBrains plugins:** classic 0.2.729, IDE 262 0.2.730, IDE 263 0.2.731.

**GitHub issues:** I closed GH #321 and #322 with release comments; #323 was already closed. I struck its queue items: the GH 320 option-B work, the surface-path move, #321, #322, #323, `sessioncrashnorestart`, `clauderesumeafterclear` and its release item. `agent:status` now shows v0.35.529, and the stray duplicated paragraph and the "2" are gone.

**Follow-ups filed after their origin items, five dispatched (🚧), each on a worktree based on `integrate/batch0530`:**
- GH 324 gap: a re-run after the source was already stripped still reports `already_applied` when the target lost turns.
- GH 325 sibling: `recover-lost` uses its own archive path join, so it can miss archived ids.
- GH 327 residue strike: a loop item is struck from a stale echo, or from an echo about another queue's item.
- Crash auto-restart of a Claude supervisor resumes the session id from before `/clear`.
- GH 326 phase 2: surface projection behind the derivation API.

**GH 326 phases 3, 4 and 5 are queued but not dispatched.** Each builds on the one before it and touches the same plugin files, so I'll dispatch phase 3 after phase 2 merges.

**v0.35.530 (your item "release + publish: Include PR #324-#327", 🚧 coordinator-owned):** the batch has #324–#327 plus the supervisor-heartbeat fix, and `make check` passed (11,243 of 11,243). It waits on the CI auto-clear race fix (🚧).
<!-- agent:boundary:4c518cb0 -->
<!-- /agent:exchange -->

<!--
-->

## Queue: Main
<!-- agent:queue id="main" subagents preset="#spec-test-build-install-commit-push" priority go -->
- ~~GH 320 follow-up (option B): real Remote Dev split geometry. The 262/263 frontend module publishes the client measured editor-window columns (bump the shared surface snapshot contract) and the backend forwards them into the controller surface Source as editor order; the remembered-order fallback, labelled unknown, stays for classic and one-sided installs. Needs live JetBrains Client verification.~~
- ~~Move Run Agent Doc and Sync Tmux Layout off the --col CLI shell-out onto the Lazily surface path (specs/13-state-backbone.md "State Propagation, Not Effect Inference").~~
- 📥 #gh-fix https://github.com/btakita/agent-doc/issues/321
- 📥 #gh-fix https://github.com/btakita/agent-doc/issues/322
- ~~do [#sessioncrashnorestart]~~
- 📥 #gh-fix https://github.com/btakita/agent-doc/issues/323
- 🚧 release + publish: Include PR #321-#323
- ~~do [#clauderesumeafterclear]~~
- 🚧 crash auto-restart of the Claude supervisor still resumes the pre-/clear session id held in supervisor memory; re-read the SessionStart-recorded resume id before respawn
- 📥 #gh-fix https://github.com/btakita/agent-doc/issues/324
- 🚧 focused extract follow-up (GH 324 gap): when the source was already stripped, the transfers.is_empty() path still reports already_applied via target_covers_selectors, which counts an id covered if any list mentions it; verify the target against the recovery copy under .agent-doc/archives/extract/ and its recorded node ids
- 📥 #gh-fix https://github.com/btakita/agent-doc/issues/325
- 🚧 queue_recovery done_archive_paths joins the raw archive= value onto the document dir, bypassing resolve_done_archive_target (same class as GH 325), so recover-lost can miss archived ids; move it onto the shared reader and fix its archive=done.md tests
- 📥 #gh-fix https://github.com/btakita/agent-doc/issues/326
- 🚧 GH 326 phase 2: move EditorTabSyncListener.captureSurface surface projection behind agent_doc_editor_derive_json as a surface_projection derivation on the delivery worker (visible-document union incl. GH 320 Remote Dev union, readiness, active-document resolution, open-tab proximity, stale-selection settling, payload incl. column_order)
- GH 326 phase 3 (after phase 2): move manual Sync Tmux Layout --col snapshot column decisions (exactVisibleSyncDecision, undetectedLayoutColumns, normalizeEditorLayout, absolutizeEditorLayout, chooseSyncProjectRoot) behind a derivation and take that layout read off the EDT
- GH 326 phase 4 (after phase 3): move RemoteClientSurfacePayload.build and the uniqueRemoteSplitSelection Kotlin remote fallback behind derivations, then retire the Kotlin fallback
- GH 326 phase 5 (after phase 4): audit pure decisions in CrdtReplicaManager, PatchWatcher and CpRouteClient (CrdtReplicaManager changed in 14 releases this month) and move them behind derivations
- 📥 #gh-fix https://github.com/btakita/agent-doc/issues/327
- 🚧 preflight residue strike must not strike a loop head from an echo written before the current pass opened or about another queue head (GH 327 pass 4 wasted a pass)
- 🚧 release + publish: Include PR #324-#327
- 📥 [from:operator] subagent: fix live Codex supervisor reported heartbeat-stale and no_longer_live (sdk-dev pane %22 stayed in-pane under Blocked by hook)
- 🚧 [from:coordinator] fix supervisor auto-clear baseline taken after item one's commit on slow startup (CI 39463326025 queue_auto_clear_types_clear_between_items_into_the_same_harness)
- 🚧 release + publish: Include PR #324-#327
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
