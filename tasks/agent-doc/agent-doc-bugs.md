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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261015-162943.md`*

Compacted content:
- Archived 9 response topic(s): queue; v0.35.506 released; #266 closed; fork-dup + compact false positive on main; todo completion API, todo duplicate-on-done, salient-response lint; 6 more
- Prior summary/context: prior compacted content: Archived 16 response topic(s): dispatched #gh-fix 261–264 + release batch; Subagent complete...

### Re: GH #268–#273 + queue duplication — dispatched to 7 subagents — opus-5-5 · 2026-10-15T12:45-04:00

Every queued fix is claimed and running in its own background subagent. Each one has its own worktree under `agent-loop-worktrees/`, based on `origin/main` (`d5f4cf9743`).

| Claim owner | Item | Branch |
|---|---|---|
| `subagent:gh268` | #268 layout repair never releases `window-size manual` pin | `fix/gh268` |
| `subagent:gh269` | #269 orphaned-preparing reaper SIGTERMs the draining predecessor | `fix/gh269` |
| `subagent:gh270` | #270 re-exec'd supervisor leaves the PTY unserviced on `state.db` lock | `fix/gh270` |
| `subagent:gh271` | #271 /clear "not proven": statusLine row + "Update installed" notice | `fix/gh271` |
| `subagent:gh272` | #272 Run while typing refused on transient `missing_replica` | `fix/gh272` |
| `subagent:gh273` | #273 held splice batch forks the buffer; rebuild-on-refusal loop | `fix/gh273` |
| `subagent:queuedup` | queue content duplicated on Run Agent Doc (formal-logic fix) | `fix/queue-dup-invariant` |

**Queue duplication (what I saw this turn):** the six `#gh-dix…` lines you typed were then corrected to `#gh-fix …`. The preflight for this Run listed **12** queue heads: both the pre-correction and the post-correction text were live at once. Shortly after, the queue block appeared twice in HEAD and on disk. I've passed that timeline to `subagent:queuedup`. It is to find the writer that produced the doubled component from the logs and say why earlier fixes didn't hold. It will also state the invariant formally, model it in TLA+ (counterexample before the fix, pass after), and enforce it at the single write choke point with regression tests for this exact shape.

**Claim anomaly (also sent to that subagent):** editing the #270 line from `agent-foc` to `agent-doc` kept its claim, but the same edit on the #272 line dropped it. I re-claimed #272 for `subagent:gh272`, so no duplicate worker was started. I treated the `agent-foc` lines as the same six issues throughout.

**Next:** `release + publish` stays queued for me rather than a subagent, because subagents may not tag or release. As each subagent reports, I'll integrate its branch serially onto `main`, re-running the plugin-version bump to resolve `gradle.properties` conflicts. Then I'll run one `make install` for the batch and close the six GitHub issues and their queue items. Finally I'll cut and publish the release, which will also carry `d5f4cf9743` (the Clear Session Context notice fix).

### Re: GH #268–#273 + queue duplication — all merged, v0.35.509 release running — opus-5-5 · 2026-10-15T14:50-04:00

All seven subagents have reported, and every branch is merged into `batch-gh268-273` (pushed). Two more fixes found along the way are merged too. `make release` for **v0.35.509** is running now. It runs one full `make check` and `make tmux-ci` on the version-bumped tree, then pushes `HEAD:main` + `v0.35.509` and installs.

| Item | Fix | Commit |
|---|---|---|
| #268 | `window-size manual` pin released when no attached client sees the window at its pinned size | `1664e19a25` |
| #269 | Both orphan-reaper scans honour the drain lease before any age test | `b2ffd1a03b` |
| #270 | A re-exec'd supervisor bridges its PTY from the first line of `main` until takeover | `e947cb3cb0` |
| #271 | `/clear` proof treats rows below the closed composer frame as footer, by position | `22233f6c40` |
| #272 | Run while typing waits ≤20 s for a transient `missing_replica`, then admits on the editor buffer | `9e79cfd138` |
| #273 | A held splice batch beside a canonical change merges forward (guarded three-way); replica rebuilds bounded per content | `a39a004177` |
| Queue duplication | Admission keeps the editor's version of any component both editor and disk changed | `d4313984b5` |
| `state.db` overwrite (found under load) | `controller serve` closes only fds it can prove it inherited | `5c2da24c03` |
| preflight-io flakes (found under load) | A controller that dies at launch is reported, not hidden as a timeout; unreadable rearm evidence fails closed | `6f54959182` |

**Queue duplication, the mechanism:** when you pressed Run, the editor had delivered the `#gh-dix` lines while its save on disk already held `#gh-fix`. Admission's three-way merge treated those as two writers and wrote both: 5101 = 4411 + 342 + 348 bytes, so preflight listed 12 heads. Your late correction keystrokes then doubled the whole block. Every earlier fix tried to recognise "same item" from the text, and each missed the next shape. The new rule needs no such guess. TLC passes the new rule (9,656 distinct states), and must-violate configs reproduce this incident and GH #198 on the old merge.

**`state.db` corruption, now proven:** `controller serve` closed every fd above stdio, including its own debug-log file. SQLite reused that number for `state.db`, and the queued `tracing initialized` line landed at byte 0 ("file is not a database"). This is the recurring production incident in memory. The flaky test reproduced it 9 of 600 times on an optimized pre-fix build, and 0 of 600 with the fix.

**Process notes:** the claims expired after 2 h, so this cycle's preflight listed every item for dispatch again. I re-claimed them for the batch rather than starting new workers. I'll close the issues, the queue items and the release item once `make release` finishes; the queue-duplication commit and #273 close on merge to `main`.
> **Chat prompt (#chatprompt):** is the release published?

### Re: is v0.35.509 published? — opus-5-5 · 2026-10-15T14:58-04:00

Not yet. Tagging and the push to `main` are done; the GitHub build and publish are still running.

- **Local gates passed:** `make check` on the version-bumped tree ran 10662 tests, all passed (168 skipped). `make tmux-ci` passed, and the release check reused its proof.
- **Tag and `main`:** `v0.35.509` and `main` both point to `62cfd2af54`, so all nine fixes are on `main`.
- **GitHub Release run `38788644925`:** the `plugins` job passed. All six platform builds are still running (about 9 min in), and publishing to GitHub Releases and PyPI comes after them.
- **Latest published:** both still show **0.35.508**.
- **Local install:** still compiling.

When the run is green I'll close GH #268–#272. #273 should close on its own from its `Fixes` commit, and I'll check that. Then I'll close the queue items and add the follow-ups to the backlog.

### Re: v0.35.509 integrated, GH #268–#273 closed — opus-5-5 · 2026-10-15T15:05-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/268

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/269

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/270

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/271

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/272

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/273

> **Queue prompt:** The content of the queue were duplicated by agent-doc on Run Agent Doc. This has been a reoccurring issue with multiple fix attempts. Use formal logic to prevent this from happening again.

All seven are fixed, on `main`, and tagged in **v0.35.509** (`62cfd2af54`). The fixes, mechanisms and test evidence are in the previous response and in `VERSIONS.md` 0.35.509.

- **`make release`:** exit 0. `make check` passed on the version-bumped tree (10662 tests, 0 failed, 168 skipped). `make tmux-ci` passed. The tag and `main` were pushed, and the local `install-full` completed, so `agent-doc --version` now reports 0.35.509.
- **GitHub issues (#gh-fix "fix then close"):** I closed #268–#272 with each one's commit and the release. #273 closed itself from its `Fixes` commit.
- **Still open:** `release + publish`. Release run `38788644925` is still building the platform archives (`plugins` passed). GitHub Releases and PyPI still show 0.35.508. I'll close that item once the run publishes.

**Follow-ups filed** (appended to the backlog and queue, behind `release + publish`), all found by the subagents:
- `keylogrebase`: after a compaction the keystroke log keeps the old base hash. Fixing it also closes the documented gap where a disk-only component can still duplicate.
- `claimfollowedit`: editing a claimed queue line drops its claim, which is what happened to #272.
- `splicesessionack`: the #273 remainder, where session-check still says "acknowledgement pending".
- `rearmreadfailclosed2`: two more rearm-evidence reads that treat a failed read as no evidence.
- `execfailpipe`: the exec-time fd close may also close Rust's exec-failure pipe. Needs verification.
- `staleplugwarnflake`: one more flaky test under load.

### Re: follow-ups + GH #274 — dispatched to 7 subagents — opus-5-5 · 2026-10-15T15:12-04:00

All six follow-up items and the newly queued GH #274 are claimed. Each runs in its own background subagent and worktree, based on `origin/main` = v0.35.509 (`62cfd2af54`).

| Claim owner | Item | Branch |
|---|---|---|
| `subagent:keylogrebase` | Keystroke log keeps the pre-compaction base hash; then close the disk-only Residual gap | `fix/keylogrebase` |
| `subagent:claimfollowedit` | Claims bind to the queue item's identity, so an edit carries the claim; ambiguous duplicates are refused | `fix/claimfollowedit` |
| `subagent:splicesessionack` | session-check reads the #273 rebuild-exhausted state and names the operator action | `fix/splicesessionack` |
| `subagent:rearmreadfailclosed2` | Remaining rearm-evidence reads fail closed | `fix/rearmreadfailclosed2` |
| `subagent:execfailpipe` | Verify whether the pre-exec fd close hides exec ENOENT; fix if confirmed | `fix/execfailpipe` |
| `subagent:staleplugwarnflake` | Find and fix the load flake's nondeterminism | `fix/staleplugwarnflake` |
| `subagent:gh274` | GH #274: non-ASCII → `?` projection + duplicate committed response committed by preflight repair | `fix/gh274` |

For #274, the subagent first checks whether 0.35.509 already blocks the sequence, since the #273 merge-forward and cut-exclusivity rules are both new. It then fixes what remains at two choke points: byte-exact UTF-8 projection that fails closed instead of substituting, and a repair commit that refuses a duplicate committed section.

**`release + publish` (v0.35.509):** the tag and `main` are pushed and the local install is done. Release run `38788644925` is still building platform archives, and GitHub Releases and PyPI still show 0.35.508. I'll close this item once it publishes. This batch goes into the next release.

### Re: `agent:todo` id + done flow — dispatched — opus-5-5 · 2026-10-15T15:16-04:00

The new `agent:todo` item is claimed for `subagent:todoids`, branch `feat/todo-ids` from v0.35.509. It is scoped to three parts:

1. **Id discipline.** A todo reuses the id of the review or backlog item it refers to, and its text becomes that item's actionable operator prompt. Otherwise it mints a new id through the same coined-id rules. A todo with no id, or with an id that resolves to nothing, is handled at write time, and duplicates for one id fold.
2. **Operator done flow.** Checking the box (`- [x]`) in the editor is the done signal. agent-doc reconciles it without an agent turn (moved to done, closing the linked review gate where appropriate), and no later agent write may revert the operator's check.
3. **Agent auto-check.** At preflight and closeout, todos whose completion the binary can check cheaply and deterministically are completed with a recorded reason, for example a linked id that is already done or a referenced release that exists. Anything that needs human judgement is left for the operator.

**In flight:** 8 subagents (6 follow-ups, GH #274, and this one). **`release + publish`:** run `38788644925` has 5 of 6 platform builds and `plugins` green. The `x86_64-apple-darwin` and Windows builds are still running, and it is not published yet.
> **Chat prompt (#chatprompt):** can we fix the tmux pane layout in the runtime? if yes, fix it

### Re: tmux pane layout fixed in the runtime — opus-5-5 · 2026-10-15T16:40-04:00

Yes, and it's fixed. Window `@2` (`agent-doc`) now shows `%203` (agent-doc-bugs.md) and `%204` (fpe.md), which matches the editor's latest split. It held unchanged for 60 s after the fix.

**Cause:** two controllers were fighting over the same tmux window.
- At 19:57:00Z, during your 2→1→2 pane switch under heavy load, the **haiven-dev** controller published a target layout of `[sdk.md, sdk.md]`, the same document twice. That layout can never be satisfied with one `sdk.md` pane.
- It retried every 5–7 s for **292 attempts** (`observed_panes=unknown`, `pane_count_mismatch`). Each attempt re-applied that layout and moved `%203` into the `stash` window.
- Meanwhile the **agent-loop** controller kept converging to the correct `[agent-doc-bugs.md, sdk.md]` (generations 36–38). Your editor publications only reached the agent-loop controller, so nothing replaced haiven-dev's stale target. That's why Sync Tmux Panes only helped briefly and restarting IDEA didn't fix it. The 5 s retry storm may also have added to the CPU load.

**Runtime fix:** `agent-doc admin recycle --project-root src/haiven-dev`, recycling only that one controller. The stale target existed only in that process (`plane_version=none`), so the retry loop stopped at once (last attempt 20:30:16Z) and the agent-loop controller's layout now holds. I tried `agent-doc sync --exact-visible --no-autostart` first, but its safe passive mode refused, treating `sdk.md` as unresolved.

**Note:** `%180` (sdk.md's supervisor, titled `⚠ STALE SUPERVISOR`) is in the stash window because your editor's second split now shows `fpe.md`. That is correct, not a symptom.

> **Chat prompt (#chatprompt):** can I safely start defining todo ids in various agent docs?

### Re: todo ids — yes, with this shape — opus-5-5 · 2026-10-15T16:40-04:00

Yes. v0.35.509 already treats todo ids as first-class:
- **Format:** `- [ ] [#id] <action>`, the shape `agent-doc write --todo-add` emits. `id=<custom> ` chooses the id.
- **Completion:** `--todo-done/--todo-edit/--todo-reopen/--todo-remove` select an item by its own id, by a `#id` it cites, or by exact text. Duplicate copies fold.
- **Reusing an id:** to point a todo at existing work, either use that review/backlog item's id as the todo id or cite it (`[#todo1] confirm … for #ahfe`). Both resolve today. The in-flight `feat/todo-ids` subagent standardises on reusing the review/backlog id, so prefer that for new ones.
- **Two caveats:**
  - Keep ids unique within a project, since the same id in two components is read as the same item.
  - session-check's coined-id warning only looks in backlog/queue/done/review, so mentioning a todo-only id in a response may print a cosmetic `coins new id(s)` warning until `feat/todo-ids` lands.

> **Chat prompt (#chatprompt):** subagent: fix agent-doc causes of the old haiven-dev controller remaining after a hot update

### Re: stale controller after hot update — dispatched — opus-5-5 · 2026-10-15T16:40-04:00

Dispatched to a background subagent on branch `fix/stale-controller-hot-update` from v0.35.509, with the evidence above. Its scope:
1. Why the install-full runtime recycle left haiven-dev's controller (pid 2143461, started 19:04:44Z) running: nested project roots, skip reasons, or a guard that never re-arms. Also whether that controller was stale code or current but wedged. No skip may become a silent permanent survivor.
2. The layout defects it exposed:
   - Reject or normalise a `[X, X]` layout.
   - Bound a non-converging projection retry.
   - Never move or stash a pane that another project's controller owns.
   - Have editor publications supersede stale generations across controllers.
3. Why `%180`'s supervisor stayed `⚠ STALE SUPERVISOR`.

It also takes ownership of your queued "tmux auto sync … back to 1 pane" item. The 11 queue items you added during this turn get dispatched next cycle.
<!-- agent:boundary:2e8e3946 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built
<!-- agent:queue subagents preset="#spec-test-build-install-commit-push" priority go -->
- 🚧 #gh-fix https://github.com/btakita/agent-doc/issues/274
- 🚧 `agent:todo` items should either reuse an existing id or mint a new id. For example, if an item is in review, the todo should use the review item id with an actionable prompt. If the operator needs to take action, then there should be a good flow for the operator to declare the todo done. Ideally the agents will check if the todo items are done at opportune events.
- 🚧 do [#keylogrebase]
- 🚧 do [#claimfollowedit]
- 🚧 do [#splicesessionack]
- 🚧 do [#rearmreadfailclosed2]
- 🚧 do [#execfailpipe]
- 🚧 do [#staleplugwarnflake]
- #gh-fix https://github.com/btakita/agent-doc/issues/275: Also see new comment on issue.
- Add `agent:queue` `interval` which sets the poll interval. Add a `time` attribute to set time of day. Allow multiple times of day. Make the loop efficiently run in the Controller. Ideally a single loop. Make sure the loop does not "go rogue" and thrash the CPU. Jobs use background workers. Add a `loop` attribute which loops through the items per interval/schedule and keeps the items in the queue. `loop` without a value will loop indefinately. `loop=3` will loop 3 times. Make sure realtime updates are supported. Use lazily for state management.  
- Clear Session Context on sdk.md: 1st attempt had an error: 2nd attempt worked
- #todo
- sdk.md cleanup: delete the leftover half-typed `> **Queue prompt:** ... which is not f` echo (~line 1167; the full question is ~line 1147), and the open `[ ]` copies of #apimigrationcollide / #publicbundleadopt whose `[x]` duplicates exist: done?
- Next Remote Dev session: confirm tab switches move tmux (GH #262 fix, plugin 263 0.2.617 after v0.35.504 is installed on the backend): confirmed
- [#ahfe]: did not work I believe. Look out for bug reports
- idea typing is very laggy. 100% CPU usage. Due to build? Can we keep Idea snappy under heavy load?
---
sdk.md issue: I asked the agent to add ids on the todo items:
```
❯ /agent-doc /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk.md

● Bash(agent-doc session-check tasks/sdk.md 2>&1 | grep -v '^\[perf\]' | cut -c1-200; git --no-pager diff --stat
      tasks/sdk.md | cat)
  ⎿  [session-check] INTERRUPTED: open backlog item(s) exist only outside live agent:backlog: #tdidsasstrings (lin
     e 51), #tdadr0026 (line 52), #tdrobloxaccess (line 53), #tdclireview (line 54), #tdlyraplay
     tasks/sdk.md | 2 ++
      1 file changed, 2 insertions(+)

● Nothing has changed, so re-invoking will keep hitting the same refusal. I need a decision from you to get past it.
───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
 ☐ Unblock

│ Preflight keeps refusing tasks/sdk.md because the guard reads the [#id] tags on open agent:todo items as stray
│ backlog items. How should I unblock it?

❯ 1. Fix the guard (Recommended)
     Change the orphaned-backlog check in agent-doc (src/agent-doc in agent-loop) so it skips ids that sit in
     agent:todo. Add a regression test, run make check, make install, then re-run /agent-doc tasks/sdk.md.
  2. Strip todo [#id] prefixes
     Remove the [#id] prefixes from the 21 open todo items so preflight admits the turn. The two queue lines use
     #tdsdkdevrename / #tdue427go, so they lose their id link.
  3. File an issue only
     File the false positive against btakita/agent-doc with `agent-doc admin issue` and change nothing else. The
     session stays blocked.
  4. Type something.
───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
  5. Chat about this
```
---
- Does the queue have a way to represent the equilavent of `[/]`? `~...~` is equivalent to `[x]`.
- tmux auto sync got out of sync when switch the editor from 2 panes to 1 pane back to 2 panes under heavy cpu load. Sync Tmux Panes temporarily brings tmux to 2 panes but agent-doc incorrectly auto syncs back to 1 pane. Restarting Idea did not fix the issue.
- 🚧 release + publish
<!-- /agent:queue -->

## Todo

<!-- agent:todo -->
- [ ] Next Remote Dev session: confirm tab switches move tmux (GH #262 fix, plugin 263 0.2.617 after v0.35.504 is installed on the backend)
- [ ] sdk.md cleanup: delete the leftover half-typed `> **Queue prompt:** ... which is not f` echo (~line 1167; the full question is ~line 1147), and the open `[ ]` copies of #apimigrationcollide / #publicbundleadopt whose `[x]` duplicates exist
- [ ] [#ahfe] Live-check GH #267 on the 263 IDE: after `agent-doc upgrade` WITHOUT restart, once idea.log shows `[native] hot-reloaded libagent_doc`, Run Agent Doc should complete (ops.log `editor_route_superseded_editor_admitted`, route output ends with the advisory line). This is the first route ever sent to an old plugin against a new binary; report it if it stalls.
<!-- /agent:todo -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#todobracketguard] Preflight shadow-backlog guard (`detect_shadow_open_items`, agent-doc-element-backlog/src/backlog.rs:2391, raised from agent-doc-preflight-io `enforce_no_shadow_open_backlog`) refuses `- [ ] [#id]` items in `agent:todo` as "open backlog item(s) exist only outside live agent:backlog" because `todo` is not in its excluded-component list (backlog/review/icebox/exchange/queue). It hard-wedged haiven-dev tasks/sdk.md on agent-doc 0.35.509 (21 todo ids); the same `[ ] [#id]` format works on the operator coder instance, so compare versions/guard paths. Fix: exclude `agent:todo` (or parse its own `[#id]` as todo ids, not backlog ids) in the guard, add a regression test with bracketed todo ids, and reconcile with the #todo id/done-flow work (feat/todo-ids). Workaround in place: sdk.md todos use a bare `#id`.
- [ ] 🚧 [#keylogrebase] Keystroke log reopened after a compaction keeps the pre-compaction base hash (`merge_base=97b41a5fa5b6` after the 2026-10-15 16:29:43Z compaction), so admission has no op-lineage proof. Fixing it also closes the documented gap where a disk-only component can still arrive twice (see specs/14-realtime-workflow.md "Cut exclusivity" and the TLA+ `AdmissionCutExclusivityResidual` config).
- [ ] 🚧 [#claimfollowedit] Queue claims are keyed by exact line text, so editing a claimed line drops the claim (`queue_claim_prune pruned=1`; GH #272 line 2026-10-15). Claims should follow the edited item (node identity) the way steering forward_to_owner already does.
- [ ] 🚧 [#splicesessionack] GH #273 remainder: session-check still reports "acknowledgement pending" after the controller logs `retained_persistence_replica_rebuild_exhausted ... editor_holds_unmerged_visible_text`; give session-check a durable read of that state and name the operator action.
- [ ] 🚧 [#rearmreadfailclosed2] Two remaining rearm-evidence reads in agent-doc-document-realtime-io/src/lib.rs (around lines 4167 and 5334) still use `.ok().flatten()`, treating a failed read as "no evidence"; apply the fail-closed rule from fix/preflight-io-load-flakes (6f54959182).
- [ ] 🚧 [#execfailpipe] Verify and fix: `close_inherited_fds_on_exec` closes fds >= 3 in the child before exec, likely including Rust spawn's CLOEXEC exec-failure pipe, so an exec ENOENT may report as a successful spawn and defeat the launch-ENOENT retry.
- [ ] 🚧 [#staleplugwarnflake] `warnings::tests::stale_plugin_warning_end_to_end_from_reliable_registration` failed 1/96 whole-crate runs under load (2026-10-15); find and fix the nondeterminism, never rerun to green.
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
