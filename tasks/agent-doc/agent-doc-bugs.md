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

### Re: correction — todo ids are NOT safe yet on 0.35.509 — opus-5-5 · 2026-10-15T16:42-04:00

**Correction to my last answer.** The sdk.md transcript you queued shows that `[#id]` tags on open `agent:todo` items **do** break on 0.35.509. The orphaned-backlog guard reads them as stray backlog items, and preflight refuses the whole document (`INTERRUPTED: open backlog item(s) exist only outside live agent:backlog: #tdidsasstrings …`). My check only covered the `--todo-*` write flags, not the admission guards. Until the fix ships, **don't add todo `[#id]`s to other documents**. sdk.md stays blocked until then. Stripping the ids (its option 2) would unblock it now, but it loses the two queue links.

The fix went to `subagent:todoguard` (`fix/todo-id-guard`) at top priority, with the option you picked (fix the guard). It is a minimal change: todo-declared ids, and queue references to them, count as tracked in the orphan guard and in the coined-id checks. The broader `feat/todo-ids` design builds on it. I'll install it as soon as it lands.

### Re: queue items added this turn — opus-5-5 · 2026-10-15T16:42-04:00

**Dispatched to subagents** (claimed, each in its own worktree from v0.35.509, builds told to run under `nice -n 19 ionice -c3`):
- `subagent:todoguard`: the sdk.md todo-id guard false positive (above).
- `subagent:gh275`: GH #275 plus the new comment on the issue.
- `subagent:queueschedule`: `agent:queue` `interval=` / `time=HH:MM[,…]` / `loop` / `loop=N`. It runs as a single deadline-sleeping scheduler loop per controller, with a minimum interval, tick coalescing after suspend/hot-update, and back-off. Jobs go to background workers, `loop=N` pass counts are persisted, edits reschedule in real time, and state lives in lazily.
- `subagent:clearctxerr`: the sdk.md first-press Clear Session Context error. It checks whether the haiven-dev layout retry storm or the stale `%180` supervisor caused it.
- `subagent:ideacpu`: IDEA lag. It attributes the CPU (builds vs IDEA vs agent-doc), moves any plugin work off the EDT, and makes the Makefile/gradle/nextest entrypoints run at low priority with core headroom. It also writes operator guidance for keeping IDEA snappy.
- **tmux auto-sync back to 1 pane:** claimed for `subagent:stalecontroller`, which already owns that fix (the `[X, X]` layout, bounded retries, and cross-project pane ownership).

**IDEA lag, immediate relief:** I reniced the ~10 running rustc/cargo/nextest processes to +19. Load average went from 115 (15-min) to 29 (1-min).

> **Queue prompt:** #todo

`#todo` is not a defined preset (the presets are `#actionable-review`, `#agent-doc-causes`, `#do-backlog`, `#gh-fix`, `#release`, `#self`, and the two `#spec-…` ones). I read it as "process the todo answers below", and did that.

> **Queue prompt:** sdk.md cleanup: delete the leftover half-typed `> **Queue prompt:** ... which is not f` echo (~line 1167; the full question is ~line 1147), and the open `[ ]` copies of #apimigrationcollide / #publicbundleadopt whose `[x]` duplicates exist: done?

Yes, it's done. sdk.md has no `which is not f` echo, no open `[ ]` copy of `#apimigrationcollide`, and `#publicbundleadopt` remains only as its single gated `[/]` item. I checked that todo off.

> **Queue prompt:** Next Remote Dev session: confirm tab switches move tmux (GH #262 fix, plugin 263 0.2.617 after v0.35.504 is installed on the backend): confirmed

Thanks. I checked that todo off.

> **Queue prompt:** [#ahfe]: did not work I believe. Look out for bug reports

I recorded the outcome on `#ahfe` and left it open. I'll watch for bug reports before reopening GH #267, and I didn't start a fix on a suspected failure.

> **Queue prompt:** Does the queue have a way to represent the equilavent of `[/]`? `~...~` is equivalent to `[x]`.

Partly. The queue has two states of its own:
- **`~~…~~` (done):** the binary strikes the line when the item closes.
- **`🚧` (in flight):** the binary adds this prefix when a worker claims the line (`agent-doc queue claim`) and removes it on release. That is what you see on the dispatched items now.

There is no queue marker for `[/]` meaning *gated, waiting on something external*. A gated item lives in `agent:review` or as a `[/]` backlog item and leaves the queue. A queue line tagged `[operator-verify]` stays in the queue but is never drained by the agent. If you want a first-class gated queue state (for example `[/]` on a queue line that the drain skips and that re-arms when the gate clears), say so and I'll queue it.

### Re: v0.35.509 published; batch 0.35.510 started — opus-5-5 · 2026-10-15T16:50-04:00

> **Queue prompt:** release + publish

**v0.35.509 is published.** Release run `38788644925` succeeded (all 6 platform builds plus `plugins`). GitHub Release `v0.35.509` is marked Latest (19:20Z), and PyPI `agent-doc` is 0.35.509. It is also installed locally. This closes the GH #268–#273 / queue-duplication / `state.db` batch.

**Next batch (`batch-0.35.510`, pushed):** `fix/execfailpipe` (`cb30c3f66a`), `fix/rearmreadfailclosed2` (`f937a38c6c`), and `fix/staleplugwarnflake` (`1c3d056f0b`) are merged.
- **execfailpipe:** a real bug, now fixed. The pre-exec hook closed Rust's exec-failure pipe, so a missing controller binary reported a *successful* spawn, the child aborted with a core dump, and the launch-ENOENT retry never ran. It now marks fds close-on-exec (`close_range(CLOSE_RANGE_CLOEXEC)`) instead of closing them.
- **rearmreadfailclosed2:** both remaining write-path reads now refuse instead of treating a failed read as "no evidence". The error-swallowing reader is deleted.
- **staleplugwarnflake:** the test now uses the in-process relay route instead of cold-launching a controller (6/128 failures under load → 0/128).

**Found along the way, dispatched:** that flake belongs to a class. Many lib tests cold-launch a separate `controller serve` and wait a fixed ~2 s for its socket, which gave ~500 panics in `agent-doc-preflight-io` under load. The same cause, plus a 5 s wall-clock assertion, explains the `queue-io` failures seen in two subagents' `make check` runs. `fix/cold-launch-test-flakes` makes these tests in-process or condition-based and adds a guard against new cold-launching lib tests, so the next release's single `make check` is reliable.

### Re: Subagent completed: `subagent:claimfollowedit` returned work for queue item “do [#claimfollowedit]” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=aa9ac8366d408bb6575405951153e6d8e4256c2446f2a6528ab7ea864ce41399 -->
### Re: release status, and resuming the subagents a dead session left behind — opus-5-5 · 2026-10-15T17:51-04:00

> **Chat prompt (#chatprompt):** release published?

**v0.35.509 is the latest published release.** The GitHub Release is marked Latest (19:20Z), PyPI is 0.35.509, and the Release run succeeded. Nothing newer has shipped. `batch-0.35.510` is built up but not released yet (state below).

**What happened to the in-flight work:** the previous coordinator session exited, and its background subagents died with it. Their claims stayed held by a dead owner, and 8 worktrees were left with uncommitted, half-finished changes. No work was lost: every worktree still holds its diff.

**Resumed: 9 fresh subagents, one per existing worktree.** Each continues from the previous worker's diff, with the operator authorization pasted verbatim. Builds run under `nice -n 19 ionice -c3` with `CARGO_BUILD_JOBS=6` so IDEA stays responsive (load is ~3 now). Version bumps are left to integration.
- `subagent:gh274` → `fix/gh274` (GH #274, the duplicated response with `?` characters)
- `subagent:todoids` → `feat/todo-ids` (todo id reuse/mint plus a done flow; builds on `fix/todo-id-guard`)
- `subagent:keylogrebase` → `fix/keylogrebase`
- `subagent:gh275` → `fix/gh275` (GH #275 plus the new comment; builds on `fix/todo-id-guard`)
- `subagent:queueschedule` → `feat/queue-schedule` (`interval` / `time` / `loop`)
- `subagent:clearctxerr` → `fix/clear-first-attempt-error`
- `subagent:ideacpu` → `fix/keep-ide-snappy`
- `subagent:stalecontroller` → `fix/stale-controller-hot-update` (also covers the tmux 2→1→2 pane auto-sync item)
- `fix/cold-launch-test-flakes`: the test-flake follow-up found while fixing `#staleplugwarnflake`. Not a queue item.

**Integrated into `batch-0.35.510` (pushed):** `fix/todo-id-guard` (the fix for `#todobracketguard`, which unblocks sdk.md) and `fix/splicesessionack` (GH #273 remainder) merged cleanly. The batch already held `#claimfollowedit` (its completion note above), `#execfailpipe`, `#rearmreadfailclosed2` and `#staleplugwarnflake`. `make check-fast` is running on the batch now. These items stay claimed until the batch passes its single `make check` and is installed. The full check waits for `fix/cold-launch-test-flakes`, so the known flake class doesn't turn that run red. Then one `make install`, the closeouts, and the sdk.md re-check.

> **Queue prompt:** Run Agent Doc on docs.md: agent-doc should interrupt the turn until the writing has settled. The

**Deferred:** you're still typing this one (the typing gate holds it as unsettled). It is parked under a 30-minute coordinator claim, and editing the line releases it. I'll dispatch it once the sentence is complete.

The exchange is large (221 lines, 13 sections). I'm noting that here, not stopping for it.

### Re: #rundocsstop dispatched; #todo synced — opus-5-5 · 2026-10-15T18:10-04:00

**Last cycle's response landed.** The delivery that was stuck (the plugin's captured local delta blocking the visible buffer) cleared, and preflight recovered and committed it this cycle. The `fix/captured-delta-livelock` subagent is still finding the root cause, so the next heavy-load case won't loop for an hour.

**`do [#rundocsstop]` → dispatched to `subagent:rundocsstop`** (branch `fix/rundocsstop`, its own worktree from v0.35.509). Scope: find which preflight guard refused the docs.md Run Agent Doc while you were still typing. Make admission wait (bounded, reactive) for the typing gate to settle, then admit the turn and surface the edit as realtime steering, without refusing the cycle. Includes deterministic tests and a spec update.

**`#todo` (Sync agent:todo: use ids in the agent-doc todo format):** done inline. It edits this session document, which subagents may not touch. Both checked items now carry ids: `#gh262tabtmux` and `#sdkmdcleanup`. The open item already used the format (`[#ahfe]`). No item was dropped, and no state changed. New open todo ids are safe again once `fix/todo-id-guard` is installed. It is merged in `batch-0.35.510`, awaiting the batch's single `make check` + `make install`.

**In flight (all claimed):** 10 subagents: gh274, todo-ids, keylogrebase, gh275, queue-schedule (`#pollintervalattribute`; its claim followed your rename), clear-context error, IDEA snappiness, stale controller / tmux 2→1→2, cold-launch test flakes, captured-delta livelock, plus `#rundocsstop`. `#claimfollowedit`, `#execfailpipe`, `#rearmreadfailclosed2` and `#staleplugwarnflake` are already in `batch-0.35.510`. They close at the batch install.

### Re: Subagent completed: `subagent:gh275` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/275: Also see new comment on issue.” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=97b19dc240ac1cef54974e5c057324a9d94eb02148893777a16d59480e61fc24 -->

### Re: Subagent completed: `subagent:todoids` returned work for queue item “🚧 `agent:todo` items should either reuse an existing id or mint a new id. For example, if an item is in review, the todo should use the review item id with an actionable prompt. If the operator needs to take action, then there should be a good flow for the operator to declare the todo done. Ideally the agents will check if the todo items are done at opportune events.” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=7187d25387231014829d409ce008c12a413db7d72e68a24cc0077251806ef5b8 -->

### Re: Subagent completed: `subagent:keylogrebase` returned work for queue item “🚧 do [#keylogrebase]” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=4497b8046b848562916487cf8510ba9da1dc615714750f1ed0410ff20501229a -->

### Re: Subagent completed: `subagent:gh274` returned work for queue item “🚧 #gh-fix https://github.com/btakita/agent-doc/issues/274” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=9e25a44287a6f904a740ca548d3540efc2adcde78b9931ca918e7af63c9b76d8 -->

### Re: Subagent completed: `subagent:clearctxerr` returned work for queue item “Clear Session Context on sdk.md: 1st attempt had an error: 2nd attempt worked” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=0cecbcdf7ad3463470033aaa31804de8af70c26d449ba5af3fcbda2e275e5eab -->

### Re: Subagent completed: `subagent:queueschedule` returned work for queue item “do [#pollintervalattribute]” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=cfcdbc246239a444000b2ae23b98a29e084c4aef30b0e4cc7ff9f745c577cd9a -->

### Re: Subagent completed: `subagent:rundocsstop` returned work for queue item “do [#rundocsstop]” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=a8455ce3b81e60ef71eb025b00ce202b4ae179a899b0bdd0e555fa0348da133a -->

### Re: Subagent completed: `subagent:stalecontroller` returned work for queue item “tmux auto sync got out of sync when switch the editor from 2 panes to 1 pane back to 2 panes under heavy cpu load. Sync Tmux Panes temporarily brings tmux to 2 panes but agent-doc incorrectly auto syncs back to 1 pane. Restarting Idea did not fix the issue.” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=26c1061762af31bf3327338bb0096e34d3dcde0f7e008ee15c89db413a25cc24 -->

### Re: Subagent completed: `subagent:ideacpu` returned work for queue item “idea typing is very laggy. 100% CPU usage. Due to build? Can we keep Idea snappy under heavy load?” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=6c963ebb93240c7ec4b29be66244d98388e3f8068257c7c726c843784a246121 -->
> **Chat prompt (#chatprompt):** cont

### Re: batch-0.35.510 integrated, checked, installed, pushed to main — opus-5-5 · 2026-10-15T19:50-04:00

> **Queue prompt:**
>
> do [#pollintervalattribute]
>
> do [#rundocsstop]
>
> do [#keylogrebase]
>
> do [#claimfollowedit]
>
> do [#splicesessionack]
>
> do [#rearmreadfailclosed2]
>
> do [#execfailpipe]
>
> do [#staleplugwarnflake]


**Done.** Every finished subagent branch is merged into `batch-0.35.510`.
- **One full `make check`:** exit 0, 10790/10790 tests passed. TLA+ was skipped because its models are identical to a run that passed earlier.
- **One `make install`:**
  - JetBrains 0.2.642 hot-upgraded in the live IDE. 262 0.2.643 was written to Rider and loads at its next start. 263 is 0.2.644.
  - VS Code 0.2.82 installed.
  - 27 supervisors and 3 controllers recycle at their next idle boundary.
- **`main`:** fast-forwarded to `8cc279f603`. CI for that push is still running; I recorded that rather than waiting on it.
- **GitHub:** GH 274 and GH 275 are closed with fix commits. No tag yet: this is a local install, and the release is the next step whenever you want it.

**What landed**
- **GH 274 (`?` duplicate response):** the JetBrains plugin passed native strings in the JVM locale. An ASCII-locale backend turned non-ASCII into `?`, and repair later committed that copy beside the real one. Now UTF-8 is explicit and strict at the JNA boundary. The projection and repair also refuse a second exact copy, or a lossy copy, of a committed response.
- **GH 275 and the `agent:todo` guard:** todo rows are no longer shadow backlog. A committed closeout whose session check still refuses now exits nonzero instead of printing "superseded".
- **Todo ids:**
  - `--todo-add` reuses the open review/backlog id it cites, or mints a fresh id.
  - Checking the box is the operator's done signal, and no agent write unchecks it.
  - Preflight reports todo signals, and closeout auto-checks a todo only when its own id is resolved.
- **`keylogrebase`:** a keystroke log reopened after compaction now restarts on its real base. Admission trusts it only when replay reproduces the recorded tip. This closes the disk-only double-arrival gap, and the TLA+ residual config now passes.
- **`rundocsstop`:** a Run Agent Doc or operator edit is admitted past shadow-backlog rows, with a repair directive, instead of having its turn stopped. The closeout guard still flags the rows.
- **Queue scheduling:**
  - `interval=`, `time=HH:MM[,…]`, `loop` and `loop=N` on `agent:queue`.
  - One sleeping scheduler per controller, with a 60 s floor and a 300 s cap. Missed ticks coalesce, and failures back off.
  - Pass counts survive restarts. Attribute edits reschedule live.
- **Clear Session Context:**
  - The first press was correctly queued behind a not-yet-started Run, but the notice described a live turn. It now says the clear runs after that Run.
  - JetBrains and VS Code no longer "cancel" the Run, which only dropped its outcome and allowed a second trigger. VS Code had the same bug, and I fixed it there too.
- **Stale controller / tmux 2→1→2:**
  - `[X, X]` layouts collapse to one column, and projection retries are capped at 12.
  - A per-window claim stops an older layout from another project's controller overriding a newer one.
  - The stale-supervisor marker on `%180` was five weeks old (pane ids get reused); markers now require a live writer.
- **Captured-delta livelock:** a typing burst whose acknowledgement was lost under load froze the plugin's baseline. The response then sat in the replica but never reached the visible buffer. The plugin now re-sends the unacknowledged burst and advances only on proof, with a bound. Your 17:51 response did survive (it is in HEAD).
- **IDE snappiness:**
  - Build, test and gradle entrypoints run at low priority with core headroom left for the IDE.
  - Visual highlights reconcile incrementally.
  - New operator doc: `docs/development/ide-responsiveness.md`.
- **Test reliability:**
  - Lib tests no longer cold-launch a separate controller (498 launches → 0), and a guard rejects new ones.
  - Two wall-clock tests that failed every subagent's `make check` now take an injected clock. Each subagent had "fixed" one its own way; I kept the injected-clock versions.
- **Already in the batch:** `claimfollowedit`, `splicesessionack` (GH 273 remainder), `rearmreadfailclosed2`, `execfailpipe`, `staleplugwarnflake`.

**sdk.md and `#todo`:** bracketed todo ids are safe again once sdk.md's supervisor picks up the new build. The `#todo` sync was done last cycle (`#gh262tabtmux`, `#sdkmdcleanup`). This echo closes its queue head.

**In flight (next batch):** the docs.md Stop-hook item (an answer released but not persisted) is with `subagent:docsstophook` on `fix/docs-stophook-unpersisted`.

**Follow-ups added** at the end of the backlog: test binaries reaching live project sockets, integration tests still cold-launching controllers, and a recommended tightening of the shadow scan's inline-code skip.

The exchange is large (over 290 lines). Noting it, not stopping for it.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/274

Fixed and closed (above).

> **Queue prompt:** `agent:todo` items should either reuse an existing id or mint a new id. For example, if an item is in review, the todo should use the review item id with an actionable prompt. If the operator needs to take action, then there should be a good flow for the operator to declare the todo done. Ideally the agents will check if the todo items are done at opportune events.

Done: id reuse or mint, the operator check as the done signal, and preflight signals plus closeout auto-done (above).

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/275: Also see new comment on issue.

Fixed and closed. The new comment (only rows without backticks tripped the guard) is covered: `agent:todo` is now excluded as a whole component.

> **Queue prompt:** Clear Session Context on sdk.md: 1st attempt had an error: 2nd attempt worked

Fixed (above).

> **Queue prompt:** idea typing is very laggy. 100% CPU usage. Due to build? Can we keep Idea snappy under heavy load?

Yes, it was the builds. Fixed (above). The guide is `docs/development/ide-responsiveness.md`.

> **Queue prompt:** sdk.md issue: I asked the agent to add ids on the todo items:

Fixed with option 1 (fix the guard), now installed.

> **Queue prompt:** tmux auto sync got out of sync when switch the editor from 2 panes to 1 pane back to 2 panes under heavy cpu load. Sync Tmux Panes temporarily brings tmux to 2 panes but agent-doc incorrectly auto syncs back to 1 pane. Restarting Idea did not fix the issue.

Fixed (above). Restarting IDEA could not help: the stuck controller was in haiven-dev, not the IDE.

> **Queue prompt:** #todo

Done last cycle.
### Re: batch-0.35.510 integrated, checked, installed, pushed to main — opus-5-5 · 2026-10-15T19:50-04:00

**Done.** Every finished subagent branch is merged into `batch-0.35.510`.
- **One full `make check`:** exit 0, 10790/10790 tests passed. TLA+ was skipped because its models are identical to a run that passed earlier.
- **One `make install`:**
  - JetBrains 0.2.642 hot-upgraded in the live IDE. 262 0.2.643 was written to Rider and loads at its next start. 263 is 0.2.644.
  - VS Code 0.2.82 installed.
  - 27 supervisors and 3 controllers recycle at their next idle boundary.
- **`main`:** fast-forwarded to `8cc279f603`. CI for that push is still running; I recorded that rather than waiting on it.
- **GitHub:** GH 274 and GH 275 are closed with fix commits. No tag yet: this is a local install, and the release is the next step whenever you want it.

❯ **What landed**
- **GH 274 (`?` duplicate response):** the JetBrains plugin passed native strings in the JVM locale. An ASCII-locale backend turned non-ASCII into `?`, and repair later committed that copy beside the real one. Now UTF-8 is explicit and strict at the JNA boundary. The projection and repair also refuse a second exact copy, or a lossy copy, of a committed response.
- **GH 275 and the `agent:todo` guard:** todo rows are no longer shadow backlog. A committed closeout whose session check still refuses now exits nonzero instead of printing "superseded".
- **Todo ids:**
  - `--todo-add` reuses the open review/backlog id it cites, or mints a fresh id.
  - Checking the box is the operator's done signal, and no agent write unchecks it.
  - Preflight reports todo signals, and closeout auto-checks a todo only when its own id is resolved.
- **`keylogrebase`:** a keystroke log reopened after compaction now restarts on its real base. Admission trusts it only when replay reproduces the recorded tip. This closes the disk-only double-arrival gap, and the TLA+ residual config now passes.
- **`rundocsstop`:** a Run Agent Doc or operator edit is admitted past shadow-backlog rows, with a repair directive, instead of having its turn stopped. The closeout guard still flags the rows.
- **Queue scheduling:**
  - `interval=`, `time=HH:MM[,…]`, `loop` and `loop=N` on `agent:queue`.
  - One sleeping scheduler per controller, with a 60 s floor and a 300 s cap. Missed ticks coalesce, and failures back off.
  - Pass counts survive restarts. Attribute edits reschedule live.
- **Clear Session Context:**
  - The first press was correctly queued behind a not-yet-started Run, but the notice described a live turn. It now says the clear runs after that Run.
  - JetBrains and VS Code no longer "cancel" the Run, which only dropped its outcome and allowed a second trigger. VS Code had the same bug, and I fixed it there too.
- **Stale controller / tmux 2→1→2:**
  - `[X, X]` layouts collapse to one column, and projection retries are capped at 12.
  - A per-window claim stops an older layout from another project's controller overriding a newer one.
  - The stale-supervisor marker on `%180` was five weeks old (pane ids get reused); markers now require a live writer.
- **Captured-delta livelock:** a typing burst whose acknowledgement was lost under load froze the plugin's baseline. The response then sat in the replica but never reached the visible buffer. The plugin now re-sends the unacknowledged burst and advances only on proof, with a bound. Your 17:51 response did survive (it is in HEAD).
- **IDE snappiness:**
  - Build, test and gradle entrypoints run at low priority with core headroom left for the IDE.
  - Visual highlights reconcile incrementally.
  - New operator doc: `docs/development/ide-responsiveness.md`.
- **Test reliability:**
  - Lib tests no longer cold-launch a separate controller (498 launches → 0), and a guard rejects new ones.
  - Two wall-clock tests that failed every subagent's `make check` now take an injected clock. Each subagent had "fixed" one its own way; I kept the injected-clock versions.
- **Already in the batch:** `claimfollowedit`, `splicesessionack` (GH 273 remainder), `rearmreadfailclosed2`, `execfailpipe`, `staleplugwarnflake`.

❯ **sdk.md and `#todo`:** bracketed todo ids are safe again once sdk.md's supervisor picks up the new build. The `#todo` sync was done last cycle (`#gh262tabtmux`, `#sdkmdcleanup`). This echo closes its queue head.

❯ **In flight (next batch):** the docs.md Stop-hook item (an answer released but not persisted) is with `subagent:docsstophook` on `fix/docs-stophook-unpersisted`.

❯ **Follow-ups added** at the end of the backlog: test binaries reaching live project sockets, integration tests still cold-launching controllers, and a recommended tightening of the shadow scan's inline-code skip.

❯ The exchange is large (over 290 lines). Noting it, not stopping for it.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/274

Fixed and closed (above).

> **Queue prompt:** `agent:todo` items should either reuse an existing id or mint a new id. For example, if an item is in review, the todo should use the review item id with an actionable prompt. If the operator needs to take action, then there should be a good flow for the operator to declare the todo done. Ideally the agents will check if the todo items are done at opportune events.

❯ Done: id reuse or mint, the operator check as the done signal, and preflight signals plus closeout auto-done (above).

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/275: Also see new comment on issue.

Fixed and closed. The new comment (only rows without backticks tripped the guard) is covered: `agent:todo` is now excluded as a whole component.

> **Queue prompt:** Clear Session Context on sdk.md: 1st attempt had an error: 2nd attempt worked

Fixed (above).

> **Queue prompt:** idea typing is very laggy. 100% CPU usage. Due to build? Can we keep Idea snappy under heavy load?

❯ Yes, it was the builds. Fixed (above). The guide is `docs/development/ide-responsiveness.md`.

> **Queue prompt:** sdk.md issue: I asked the agent to add ids on the todo items:

Fixed with option 1 (fix the guard), now installed.

> **Queue prompt:** tmux auto sync got out of sync when switch the editor from 2 panes to 1 pane back to 2 panes under heavy cpu load. Sync Tmux Panes temporarily brings tmux to 2 panes but agent-doc incorrectly auto syncs back to 1 pane. Restarting Idea did not fix the issue.

Fixed (above). Restarting IDEA could not help: the stuck controller was in haiven-dev, not the IDE.

> **Queue prompt:** #todo

❯ Done last cycle.

### Re: next batch dispatched — opus-5-5 · 2026-10-15T20:02-04:00

Four queued follow-ups are claimed and dispatched, each in its own worktree from `main` `8cc279f603` (the installed batch-0.35.510):
- `subagent:closeoutduplicatedcommit` (`fix/closeoutduplicatedcommit`):
  - the double-written 19:50 response;
  - the stray `do [#id]` echo block;
  - the `session-check` → `write --commit` → "run preflight" recovery that dead-ends;
  - `dedupe` matching duplicates modulo binary-owned echo lines.

  It hands back the exact command to remove the second copy, and I run that after installing its fix.
- `subagent:testisolationleaks` (`fix/testisolationleaks`): test binaries reaching live project sockets, `state.db`, or the install-recycle trigger. It confines them to tmp roots and adds a guard.
- `subagent:integrationtesttargets` (`fix/integrationtesttargets`): integration tests that still cold-launch a controller. They get readiness-based waits or the in-process route, plus a cold-launch guard extended to integration tests.
- `subagent:shadowbacklogscan` (`fix/shadowbacklogscan`): skip only the backtick spans, not the whole line. A read-only sweep of every live session document runs first, so the change cannot start refusing them.

Still in flight: the docs.md Stop-hook item (`subagent:docsstophook`). All five go into one next batch: one `make check`, then one `make install`.
<!-- agent:boundary:0710c595 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built
<!-- agent:queue subagents preset="#spec-test-build-install-commit-push" priority go -->
- do [#closeoutduplicatedcommit]
- ~~do [#pollintervalattribute]~~
- ~~do [#rundocsstop]~~
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/274~~
- ~~`agent:todo` items should either reuse an existing id or mint a new id. For example, if an item is in review, the todo should use the review item id with an actionable prompt. If the operator needs to take action, then there should be a good flow for the operator to declare the todo done. Ideally the agents will check if the todo items are done at opportune events.~~
- ~~do [#keylogrebase]~~
- ~~do [#claimfollowedit]~~
- ~~do [#splicesessionack]~~
- ~~do [#rearmreadfailclosed2]~~
- ~~do [#execfailpipe]~~
- ~~do [#staleplugwarnflake]~~
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/275: Also see new comment on issue.~~
- ~~Clear Session Context on sdk.md: 1st attempt had an error: 2nd attempt worked~~
- ~~idea typing is very laggy. 100% CPU usage. Due to build? Can we keep Idea snappy under heavy load?~~
~~~done
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
~~~
- ~~tmux auto sync got out of sync when switch the editor from 2 panes to 1 pane back to 2 panes under heavy cpu load. Sync Tmux Panes temporarily brings tmux to 2 panes but agent-doc incorrectly auto syncs back to 1 pane. Restarting Idea did not fix the issue.~~
- ~~#todo~~
---
🚧 docs.md issue:
```
• Blocked by hook
  └ agent-doc Stop hook found fresh unresolved exchange work for /home/brian/work/btakita/agent-loop/src/haiven-dev/
tasks/docs.md after the previous cycle was already committed. Continue THIS turn in-pane: answer "Does the page status
enum have an archived value? What are the values?" in /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/docs.md
and persist with `agent-doc finalize /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/docs.md` (or `agent-doc
write --commit /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/docs.md`). Do NOT send the final answer yet.

• The stop hook found that the status-enum result was released but not yet persisted into the session document. I’m
  recording that answer now with the official schema distinction and live Haiven evidence, then I’ll run the required
  terminal-state check.
```
---
- do [#testisolationleaks]
- do [#integrationtesttargets]
- do [#shadowbacklogscan]
<!-- /agent:queue -->

## Todo

<!-- agent:todo -->
- [x] [#gh262tabtmux] Next Remote Dev session: confirm tab switches move tmux (GH #262 fix, plugin 263 0.2.617 after v0.35.504 is installed on the backend)
- [x] [#sdkmdcleanup] sdk.md cleanup: delete the leftover half-typed `> **Queue prompt:** ... which is not f` echo (~line 1167; the full question is ~line 1147), and the open `[ ]` copies of #apimigrationcollide / #publicbundleadopt whose `[x]` duplicates exist
- [ ] [#ahfe] Live-check GH #267 on the 263 IDE: after `agent-doc upgrade` WITHOUT restart, once idea.log shows `[native] hot-reloaded libagent_doc`, Run Agent Doc should complete (ops.log `editor_route_superseded_editor_admitted`, route output ends with the advisory line). 2026-10-15 operator: did not work (believed); watch for bug reports before reopening GH #267.
- [ ] [#gq2c] After the haiven-dev supervisor recycles onto the new build, re-run Agent Doc on tasks/sdk.md: bracketed todo ids are admitted again (it currently uses bare ids as a workaround).
- [ ] [#ctg7] Under heavy load, switch the editor 2 panes -> 1 -> 2 and confirm tmux follows and stays at 2 (stale-controller fix).
<!-- /agent:todo -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#closeoutduplicatedcommit] Closeout duplicated a committed response (agent-doc-bugs.md, cycle-1792108257994, 2026-10-15 19:50-19:57 local, right after make install hot-upgraded JetBrains 0.2.642 and marked 27 supervisors / 3 controllers for recycle): respond hit live_prompt_drift_after_preflight, retained the tracked-work half, then commit 0e3f550b85 landed without the response and 8462722c53 landed TWO copies of the 19:50 Re: section (HEAD lines 328 and 427). The first copy also carries a binary-inserted Queue prompt echo of the nine reaped do-id heads that the response never wrote, which is why agent-doc dedupe reports no duplicates. Meanwhile session-check named write --commit as the recovery while write --commit refused (cycle already committed, run preflight). Fix the double materialization (same class as GH 274), the stray id-head echo, the contradictory recovery guidance, and teach dedupe to match a duplicate modulo binary-owned echo lines; then remove the second copy through the binary path.
- [ ] [#testisolationleaks] Test isolation leaks into live project roots: haiven-dev ops.log shows bare 0.35.509 clients (no build hash) connecting and being rejected for build mismatch, and supervisor install recycles fired at 19:40, 19:56 and 20:27Z with no install to ~/.cargo/bin (found by fix/stale-controller-hot-update). Find the test binaries reaching live sockets/state.db and confine them to their tmp roots.
- [ ] [#integrationtesttargets] Integration-test targets still cold-launch a CLI controller serve (~540 launches per full nextest run; fix/cold-launch-test-flakes covered lib tests only). Route them through the in-process controller or a readiness-based wait and extend the cold-launch guard to integration tests.
- [ ] [#shadowbacklogscan] [recommended] The shadow-backlog scan skips any line overlapping inline code, so a genuinely drifted open backlog row containing backticks is never caught (found during the GH 275 fix). Tighten it without refusing live documents that legitimately quote ids in code.
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
