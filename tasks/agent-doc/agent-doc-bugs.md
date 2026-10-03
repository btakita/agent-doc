---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: c47dae65-e36f-40cb-8ebb-b7c9573e3c41
  codex: 01a0fd09-a13c-75b1-aa00-0e20ce0bd034
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#actionable-review': Add actionable review items into backlog + queue
  '#agent-doc-causes': Fix agent-doc causes
  '#do-backlog': /goal do backlog items
  '#gh-fix': fix then closerelease
  '#release': release + publish
  '#self': agent-doc-bugs.md
  '#subagents': 'run the remaining items in subagents'
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: go
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261003-000203.md`*

Compacted content:
- Archived 4 response topic(s): GH #99, #100, #101 and release; GH #102; GH #104–105, queue fixes, retained delivery, tmux sync, and v0.35.440; 1 more
- Prior summary/context: prior compacted content: Archived 5 response topic(s): #gh-fix issue 95; #gh-fix issue 96; #gh-fix issue 97; 2 more

### Re: GH #106, GH #107, and release v0.35.441 — opus-5.5 · 2026-10-02T20:45-04:00

> **Queue prompt:** do https://github.com/btakita/agent-doc/issues/106

> **Queue prompt:** do https://github.com/btakita/agent-doc/issues/107

> **Queue prompt:** release + publish

**GH #106: the layout escalation no longer feeds on itself** (`0bbc15868`). When you switch tabs and the target pane is stashed, the escalation used to take the last published layout, add the document as a new column, and publish that as the next layout. Each switch then read the previous switch's output, which is why the columns climbed 1→2→3→4 within one second. Now the saved layout is republished unchanged, and only when it already contains the document. Otherwise the escalation stops and logs `cause=document_outside_retained_layout`. Because nothing is ever appended, the column count can't exceed what the editor reported, and the order can't depend on which document asked first (the issue's asks 1–4).
- New regression test `focus_only_escalations_never_grow_the_retained_layout`: three uncovered documents each try to escalate against a one-column layout, and the layout stays at one column every time. The existing retained-layout test is updated to match. Spec updated in `specs/07-session-tmux-commands.md`.
- Not covered by this change: asks 5 (log when one projection undoes another) and 6 (clean up the stale-supervisor panes piling up in stash). Both repeat GH #105 asks that v0.35.440 addressed on the accounting side. The escalation was the part that kept making them worse.

**GH #107: `agent-doc upgrade` now updates editor plugins too** (`f0797d87d`). The one-shot upgrade runs the same plugin update as `--auto`. It does this after a binary upgrade and also when the binary is already current, so re-running it repairs a workspace an earlier binary-only upgrade left out of step. If the plugin step fails, the command exits non-zero and says the release is not fully installed. If the binary upgrade itself fails, plugins are left alone so they never get ahead of the binary. Two unit tests added; spec updated in `specs/07-core-commands.md`.

**Release v0.35.441**
- `make check`: 10437 tests passed, all other gates green.
- `audit-docs` first failed because the superproject's Grok, OpenCode, Codex and Cursor skill copies still carried an older version. I regenerated them with `skill install --all` and the audit passed.
- Version bumped with `release-version`; VERSIONS.md entry and editor-parity row added (no plugin change; plugin stays 0.2.480).
- `make release` re-ran the full suite, tagged and pushed `v0.35.441`, and ran `install-full`. `agent-doc --version` now reports 0.35.441.
- GH #106 and #107 are closed by their commits.
- The Release, PyPI and CI workflows were still running when I checked. The Book workflow passed.

### Re: GH #108: JetBrains upgrade reporting — opus-5.5 · 2026-10-02T21:33-04:00

> **Queue prompt:** do https://github.com/btakita/agent-doc/issues/108

Fixed in `cf4d7eb1d` (agent-doc) and pushed. GH #108 is closed. The guard itself is unchanged; it still refuses as designed. The fixes are to how its decision is reported:

1. **The message now names agent-doc, not the IDE.** The upgrader's reason now starts with `agent-doc declined the restart-free upgrade: this JetBrains build retires plugin classloaders asynchronously (AwaitClassloaderUnloadAsyncPostReconfiguration), so there is no safe synchronous swap point; …`. The launcher (`src/plugin.rs`) also recognises the 0.35.435–0.35.441 wording, so a launcher from an older package still blames agent-doc. "The IDE refused" now appears only when the platform actually refused.
2. **A staged install no longer prints a false success line.** It used to print `Plugin installed (v<old>)`. It now prints `Plugin v0.2.480 staged (not installed) for the next IDE start; <dir> still holds v0.2.479, which stays loaded until the IDE restarts.`
3. **It says once that the loss is permanent.** A one-time NOTE states that restart-free upgrade is permanently unavailable on that build, and that the workflow is upgrade + restart, not retrying.
4. **Decisions are written to `ops.log`.** Each attempt logs a `plugin_dynamic_upgrade outcome=hot_upgraded|staged_for_restart|restart_required declined_by=agent-doc|ide|upgrader_failure` line with the version, target, and a reason that includes the pid. This ran live during `make install`: `plugin_dynamic_upgrade outcome=hot_upgraded processes=1 version=0.2.481`.
5. **The stale marker is now removed.** `retire_satisfied_restart_marker` (agent-doc-fs) deletes the staged-restart marker once an installed jar at `staged_version` is at least as new as the marker, which proves the IDE start applied it. This runs on every preflight staged check. A same-version jar older than the marker does not count, and a marker without a staged version is never touched.
6. Ask 6 (wait for the async restart notification instead of refusing) is **not done**. The report itself marked it as unestablished.

I also fixed two remedy messages in preflight and plugin_jar that still said the IDE "refused". JetBrains plugin bumped to 0.2.481.

**Verification:**
- New Rust regression tests: attribution (current and legacy wording), the staged headline plus its `ops.log` fields, and marker retirement (all three cases).
- Updated Java test `asynchronousClassloaderRetirementStagesBeforeTouchingTheLiveGeneration`.
- `make check` exit 0: nextest ran 10440 tests, all passed.
- JetBrains gradle tests, run fresh: 14 passed, 0 failed.
- `make install` hot-upgraded the live IDE.
- CI for `cf4d7eb1d` was still in progress when last checked.

### Re: release + publish (v0.35.442) — opus-5.5 · 2026-10-02T21:59-04:00

> **Queue prompt:** release + publish

**v0.35.442 is tagged and pushed. Its GitHub Release and PyPI runs are still in progress.**

- `make release-version VERSION=0.35.442` set the version everywhere; `VERSIONS.md` entry added for GH #108. Commits: `3049479ef` (release) and `4919c21cb` (editor-parity audit row).
- The first `make release` stopped at `editor-parity` because 0.35.442 had no row in `editors/release-parity.tsv`. I added the row, the checker passed, and I re-ran.
- The second `make release` ran `make check` (exit 0, 10440 tests passed), tagged and pushed `v0.35.442`, then ran one `install-full`. Installed binary: `agent-doc 0.35.442`.
- The PyPI and Release workflows for `v0.35.442` were running when last checked. I did not wait for them. Per `#ci-no-closeout-wait`, the asset check (four archives + `SHA256SUMS`) happens on the next pass.

**Queue steering picked up:**
- **GH #109, #110, #111:** each is running in its own subagent and worktree outside agent-loop (`~/work/btakita/agent-doc-wt/gh1{09,10,11}`). Each commits on its own branch with no push or install; I merge, install and close the issues as they come back.
- **Your steering note** (queue/exchange changes should be picked up as soon as an item finishes): from now on I close each item as soon as it is done, so the binary hands me your new queue and exchange edits instead of me sitting on long-running work.

### Re: GH #109 — stale-supervisor pane in a layout column — opus-5.5 · 2026-10-02T23:40-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/109

**Fixed, installed, pushed, and closed** (agent-doc `ff6d867e8`, merged on main; [GH #109](https://github.com/btakita/agent-doc/issues/109) closed).

- **The pane was legitimately in that column.** `pmt2/mr/1102.md` was a retained column, so `%66` was filling its own document's column, not another document's. The real gaps were narrower: the pane's supervisor runs a superseded binary and was neither flagged nor recycled, and its move out of stash was never explained.
- **Fix:** `layout_column_audit` checks column panes after every layout sync:
  - **Stale supervisor:** detected by comparing the supervisor's exe with the installed binary, falling back to the `⚠ STALE SUPERVISOR` title when the exe can't be read. A stale supervisor in its own column gets the existing safe-boundary recycle, at most once every 10 minutes. Nothing is killed or moved.
  - **Logging:** every promoted, foreign, or stale pick logs `layout_column_pane_selected` with its reason.
- **Remaining:** tmux-router can still fill a column with a spare pane that no document owns. The audit reports this but doesn't prevent it; preventing it needs a tmux-router change.
- **Verification:** combined `make check` over all of today's merged fixes passed 10541 tests, plus audit-docs, editor-parity, lean, and tla. `make install` hot-upgraded the live IDE to plugin 0.2.483.

### Re: GH #110 — editor route gates — opus-5.5 · 2026-10-02T23:44-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/110

**Fixed, installed, pushed, and closed** (agent-doc `be7072c2f` on main; [GH #110](https://github.com/btakita/agent-doc/issues/110) closed).

- **Gate messages:** gate 3 now names the routed document and the published column set (`routed document <path> is not one of the published layout columns (...)`), and no longer says "exact visible layout". All three gates now report the receipt reason and the columns. Gate 3 also checks stacked columns by membership.
- **Shared `--wait-for-ready` budget:** one deadline covers layout convergence first, and harness readiness gets the remainder. The route publishes once, then re-observes the layout projection while time remains instead of failing at once. An operator-owned layout still fails immediately. The CLI help states the shared budget.
- **Stale supervisor binary:** reported on the route (`controller_editor_route_supervisor_binary_stale` plus a warning in the route output), but it doesn't refuse the route, because a stale supervisor can still host a healthy harness. #109 now recycles such supervisors at an idle boundary.
- **Behaviour change:** routes without `--wait-for-ready` now allow the layout at most 15s, down from 30s.
- **Verification:** included in the combined `make check` (10541 tests passed) and `make install`.

### Re: GH #111 — focus route collapsed the layout — opus-5.5 · 2026-10-02T23:47-04:00

> **Queue prompt:** #subagents #gh-fix https://github.com/btakita/agent-doc/issues/111

**Fixed in a subagent, merged, installed, pushed, closed** (agent-doc `b6a7b48ac`, merge `00148fb7b`; [GH #111](https://github.com/btakita/agent-doc/issues/111) closed).

- **`--col` semantics:** route `--col` now carries an explicit `layout_mode`:
  - `exact`: a positive multi-column observation, which replaces the layout.
  - `ensure`: each named document gets a column, and nothing is removed.
  - Old plugins that send a single `--col` infer `ensure`. A focus route therefore keeps the retained layout (focus-only), or appends the document if it has no column. An unknown mode is refused before publishing.
- **Pure merge function:** the merge is `merge_editor_route_columns(mode, retained, route)`, ready to become the body of the planned per-publisher `Computed` for the desired layout.
- **Logging:** `pane_layout_projection` lines carry `publisher= plane_version= columns= retained_columns= observed_panes=`, and narrowing logs `pane_layout_projection_narrowed`.
- **Merge:** hand-resolved against #110 (gate budget) and #112 (column order). The route resolves the mode and merges first, then #110's gates run, and #112's ordering still applies at publication. Plugin 0.2.483.
- **Open question:** in `ensure` mode on Remote Dev, visiting several documents in turn keeps appending columns until a positive observation replaces the layout. The alternative is to have the routed document take over the focused document's column. Say if you want that.

### Re: GH #112 — column order flipped with focus — opus-5.5 · 2026-10-02T23:52-04:00

> **Queue prompt:** #subagents #gh-fix https://github.com/btakita/agent-doc/issues/112

**Fixed in a subagent, merged, installed, pushed, closed** (agent-doc `8b6c366d0`; [GH #112](https://github.com/btakita/agent-doc/issues/112) closed).

- **Cause:** the focus-first order came from the JetBrains plugin. With an undetected layout (always the case on Remote Dev), it lists documents in IntelliJ's `selectedFiles` order, which puts the focused file first. The controller took that as the editor's split order.
- **Fix:** publications now carry `column_order` (`editor` / `retained` / `unknown`). The pure function `order_layout_columns(retained, incoming, order)` resolves an unknown order against the retained layout, so focus never reorders columns and new documents append at the end. Editor-sourced order still wins. A new `pane_layout_desired_published` line logs the publisher and the incoming and resolved columns.
- **Limitation:** on Remote Dev the order is stable but can't mirror the editor, because no split geometry is available (see #97). Documented in the JetBrains SPEC. It takes effect with plugin 0.2.483 and later; the current plugin is 0.2.484.

### Re: GH #113 — upgrade reconciled plugins in the old binary — opus-5.5 · 2026-10-02T23:55-04:00

> **Queue prompt:** #subagents #gh-fix https://github.com/btakita/agent-doc/issues/113

**Fixed in a subagent, merged, installed, pushed, closed** (agent-doc `238e07997`; [GH #113](https://github.com/btakita/agent-doc/issues/113) closed).

- **Fix:** after replacing the binary, `agent-doc upgrade` runs the plugin reconcile in the newly installed executable:
  - It uses the exact installed path, never a PATH lookup.
  - It passes hidden `--reconcile-plugins-release <V> --reconcile-plugins-mode once|auto` flags, which pin the release version.
  - If the new binary fails, the upgrade fails with it. If the new binary can't be launched, the reconcile falls back to the old one with a warning. `--auto` keeps using the upgraded binary for later cycles.
- **Merge:** combined with #114's per-target summary, which now prints from the new image too.
- **Limitation:** this only helps upgrades *from* a release that contains it, and the hidden flags are now a contract between releases.

### Re: GH #114 — upgrade summary wording — opus-5.5 · 2026-10-02T23:58-04:00

> **Queue prompt:** #subagents #gh-fix https://github.com/btakita/agent-doc/issues/114

**Fixed in a subagent, merged, installed, pushed, closed** (agent-doc `7287cec74`; [GH #114](https://github.com/btakita/agent-doc/issues/114) closed).

- **Fix:** the upgrade summary is now built from per-target outcomes (hot-upgraded / installed / staged / restart-required / unchanged), so a staged target is no longer counted as "updated".
- **Permanent decline:** on a build where agent-doc declines the restart-free upgrade, the summary says `Restart <IDE> to load plugin vX; restart-free upgrade is unavailable on this build` with no "on its own" hedge. A note says the native library reloads by itself and only the plugin needs the restart. The summary is also logged to ops.log as `plugin_upgrade_summary`.
- **Merge:** resolved against #113, so the auto watcher reports from the new binary's per-target outcomes.
- **Exchange size:** this document's exchange is now 131 lines across 8 responses. I'm not compacting mid-drain without the explicit opt-in.

### Re: GH #115 — staged upgrade deleted the plugin — opus-5.5 · 2026-10-03T00:01-04:00

> **Queue prompt:** #subagents #gh-fix https://github.com/btakita/agent-doc/issues/115

**Fixed in a subagent, merged, installed, pushed, closed** (agent-doc `267f84f16`; [GH #115](https://github.com/btakita/agent-doc/issues/115) closed).

- **Root cause** (confirmed from the platform bytecode): each `installAfterRestart` appends its own delete/delete/unzip/delete block, and agent-doc staged a zip with a fixed name. The second staging, at 22:15, appended a block whose plugin-dir delete runs *after* block 1 had installed the plugin and deleted the shared zip.
- **Fix:**
  - **Staging:** the staged zip gets a unique name and is validated before an atomic rename. agent-doc writes one delete+unzip+cleanup block under the platform's script lock, replacing any earlier agent-doc staging. A file lock serializes staging into one plugins directory, and an identical pending staging is reused.
  - **After restart:** preflight and `plugin list` detect `jetbrains_plugin_install_destroyed` and `jetbrains_plugin_staging_doomed`, and the marker is kept instead of retired. `plugin update` and `agent-doc upgrade` repair a destroyed install. Plugin 0.2.484.
- **Recovery for the affected IntelliJIdea2026.3** (paths under `/home/btakita`, not run from here): `agent-doc plugin update jetbrains --plugins-dir /home/btakita/.local/share/JetBrains/IntelliJIdea2026.3`, then restart the IDE. Check first whether it was already reinstalled.
- **Verification:** the subagent's `make check` passed 10549 tests. The merge was a fast-forward. `make install` hot-upgraded the live IDE.

### Re: queue status — remaining items are claimed — opus-5.5 · 2026-10-03T00:04-04:00

All three remaining queue items are claimed, so nothing is drained inline this cycle:

- `#subagents do [#preflightdeadline]` (claimed by `subagent:preflightdeadline`): a subagent is adding the admission deadline. It caps every preflight wait to the remaining budget and returns a typed, retryable refusal at phase boundaries.
- The steering note (claimed by `subagent:closeout-steering`): live mid-turn steering is merged and installed. A subagent is closing the remaining gap: closeout, `agent-doc steering`, and the next preflight will surface queue additions the hook missed, including `#subagents` dispatch.
- `release + publish` (claimed by `parent:after-subagents`): v0.35.443 is cut after both land, so the release includes them.

Each subagent is merged, run through `make check`, and installed as it reports back. Then its claim is released and the item closes in order.

### Re: #preflightdeadline + closeout steering — merged, installed, pushed — opus-5.5 · 2026-10-03T00:30-04:00

> **Queue prompt:** #subagents do [#preflightdeadline]

> **Queue prompt:** changes to the queue & the exchange should immediately be picked up once done as steering

Both are merged on agent-doc main and pushed. Superproject pointer: `9832bf6e31`.

**Verification:**
- Combined `make check`: nextest **10573 tests passed**.
- audit-docs, editor-parity, python-bootstrap, lean, and tla all pass. audit-docs passed after `make install` refreshed the superproject's Grok skill copy.
- `make install` done. Installed: `agent-doc 0.35.442` + JetBrains plugin 0.2.484.

**#preflightdeadline** (`73f5d7930`):
- **Deadline:** preflight now carries an admission deadline (the hook's 90s budget minus a 5s margin, so 85s).
- **Clamped waits:** controller call/connect/handoff windows and the settle/observation waits are all clamped to the time remaining.
- **Refusal:** past the deadline, preflight refuses at the next phase boundary with a typed `PreflightAdmissionRefused` naming the phase. It surfaces as `cycle contract UNAVAILABLE` with a `remedy: retryable` line and is logged as `preflight_admission_deadline_refused`.
- **Repeated resolving:** the whole preflight run now shares session-check's turn-scoped document projection, which removes repeated per-guard resolves. agent-loop's ops.log showed 241 uncached resolves.
- **Part (3)**, moving the overrunning phase off the admission path, waits for evidence. No `preflight_admission_overrun` has been logged in 105 ops.log files; the new refusal line names the phase when one happens.

**Steering picked up as soon as it lands** (live steering `ab0fb89ac`, plus closeout gap `b2bf60bc3`):
- **Mid-turn hook:** the PostToolUse hook delivers settled queue/exchange edits during the turn, debounced, verbatim, and exactly once. Each item carries `address_now`, `drain_after_current`, or `subagent`.
- **Closeout report:** closeout (`respond`, `write --commit`, post-commit `session-check`) now prints any steering the hook didn't deliver. `#subagents` items appear as `DISPATCH NOW` with the claim command.
- **After close:** `agent-doc steering` keeps reporting after the cycle closes.
- **Next preflight:** new `#subagents` items are listed under `queue_subagent_dispatch` rather than run inline.
- **Claim by `#id`:** `queue claim/release --item #id` now matches `#subagents do [#id]` heads. This cycle's release by bare `#preflightdeadline` worked on the installed build.
- **Caveat:** this Claude Code session started before the hook was installed, so the hook only runs here after a restart. The closeout and preflight paths cover the gap meanwhile.

Next: `release + publish` (claim released) cuts **v0.35.443**.

### Re: release + publish — v0.35.443 — opus-5.5 · 2026-10-03T00:49-04:00

> **Queue prompt:** release + publish

**v0.35.443 is tagged, pushed, and installed.** Release commit `b5e9f9a4e`; tag `v0.35.443`; superproject pointer `135ccf074f`.

- `make release` ran the full `make check` (nextest **10573 passed**; editor-parity row added for 0.35.443), tagged and pushed `v0.35.443`, then ran one `install-full`. Installed: `agent-doc 0.35.443`, JetBrains plugin 0.2.484.
- The GitHub Release and PyPI workflows for v0.35.443 were still running when last checked. I'm not waiting on them, per `#ci-no-closeout-wait`.
- v0.35.442 finished: its Release workflow succeeded, and the GitHub Release has the four platform archives, the JetBrains and VS Code packages, `SHA256SUMS`, and `EDITOR-PACKAGES.sha256`.

**What 0.35.443 ships** (full entry in `VERSIONS.md`):
- **Steering and queue:**
  - mid-turn operator steering, with a PostToolUse hook plus closeout, `agent-doc steering`, and preflight `queue_subagent_dispatch` surfaces;
  - queue claims, and a Stop hook that renders as feedback rather than an error;
  - annotated queue heads treated as operator steering.
- **Preflight:** the admission deadline (#preflightdeadline).
- **Layout routing:** GH #109–#112.
- **Plugin upgrade:** GH #113–#115.
- **Clear Session Context:** the crash is fixed.

**New backlog item `#closeoutstrandedmsg`:**
- The previous closeout printed a self-contradictory "committed … STRANDED" report, and its own queue strikes weren't in the snapshot.
- This cycle's preflight then misread those strikes as operator intent and suppressed the drain.
- The release ran anyway, because it was the only remaining head.

**Exchange size:** 195 lines across 12 responses. I won't compact without the explicit `agent_doc_auto_compact` opt-in.

### Re: #closeoutstrandedmsg — opus-5.5 · 2026-10-03T01:12-04:00

> **Queue prompt:**
>
> do [#closeoutstrandedmsg]


Fixed in agent-doc `562ee6b68` (pushed). The fix is installed, and the superproject pointer is at `393b2004f7`.

**Root cause (from ops.log, cycle-1791001397823).** The closeout's two queue strikes arrived after its own commit:
- `--done` struck `#subagents do [#preflightdeadline]` with a plain `~~…~~` (+4 bytes).
- `#ftstrike` struck the steering note with its annotation (+53 bytes).
- Both were projections deferred until editor delivery. The commit ran at 04:30:17 against the 28197-byte snapshot, and the strikes landed at 04:30:19–20.

The commit path already has a late-strike reconciler (`late_answered_free_text_strike_capture`). It only accepted HEAD + the *all-annotated* `#ftstrike` projection (+106 bytes), so the real +57 mix never matched. The retry commit therefore refused the binary's own strikes as STRANDED operator drift. `respond` then reported "committed" and quoted that STRANDED remedy in the same message. The strikes stayed out of HEAD, and the next preflight read them as `user_intent_prompt_changes`.

**Fix:**
1. `agent-doc-queue`: new `matches_answered_strike_in_any_projection_shape`. It accepts HEAD with exactly the answered nodes struck, each either plain or `#ftstrike`-annotated. A strike of an unanswered head, a new prompt, or a missing strike is still refused.
2. `agent-doc-commit-io`: the late-strike reconciler uses it, so the binary's own late strikes commit forward with the `commit_reconciled_late_answered_free_text_strike` proof.
3. `agent-doc-repair-command-io`: a settled closeout now prints only `[respond] closeout committed: …`. The superseded error and its remedy go to ops.log only (`strict_closeout_error_settled_committed`).

**Verification:**
- New tests: `commit_recovers_late_mixed_done_and_free_text_strikes` (commit-io), `answered_strike_accepts_plain_and_annotated_mix_only` (queue), `recovered_closeout_report_states_terminal_outcome_only` (repair-command-io).
- With the fix disabled, the commit test fails with the exact incident refusal, so it guards the fix.
- `make check`: 10576 passed, exit 0. `make install` done.
- CI run 37098921991 was in progress at closeout.

> **Operator note:** also fix in subagent (lazily.md repeated no-op preflight)

A background subagent is working on this in its own worktree outside agent-loop. The report shows preflight finding no changes 4 times in 30 minutes while the operator was editing, and telling the operator to "save the file". I'll report its root cause and fix when it finishes.

### Re: remove `queue: stop` — opus-5.5 · 2026-10-03T02:08-04:00

> **Queue prompt:** We need to remove `queue: stop` entirely. This is too confusing.

Done in agent-doc `8aec612ec` (pushed and installed; the superproject pointer is `3537aaf30e`).

**The new model (`#queuestopremove`):** a queue is either **running** or **held**.
- **Running:** no control, `go`, or `start`. No control is the default `go`.
- **Held:** `queue: pause`, the only hold. Delete the line, or write `go` on the marker, to run it.

**What changed:**
- **Drains and halts** now clear the control. Nothing writes `stop`, and `merge_queue_control` refuses it. This was the lazily.md bug: a halt wrote `queue: stop` and `do [#lzwiremodel]` sat there with no visible reason.
- **Holds the binary has to keep** now write `queue: pause` through the new `merge_queue_hold`: an operator `--- stop` fence it consumed, a wedged owner pane, and an operator command that preempts a drain. When that command finishes, the preempted queue goes back to the default `go`.
- **Legacy input is migrated, never honored as a hold:**
  - Frontmatter `queue: stop` and a lone `queue_active: false` read as no control and disappear on the next write.
  - A `stop` token on the `agent:queue` marker is your hold gesture, so it reads as `pause`.
- **What happens to the 20 tracked docs still carrying `queue: stop`:** only `tasks/software/lazily.md` has a live head. `do [#lzwiremodel]` will now run when that document's next preflight runs. None of the 20 mirror their backlog into the queue.
- **Related fixes:**
  - Frontmatter parsing now accepts an empty `---`/`---` block. Clearing the only key leaves exactly that, and it used to be rejected.
  - The no-changes guidance from `#noopnamefault` now says "held (`queue: pause`) — delete that line".
  - The specs and README describe `pause` as the one hold. The `--- stop` queue fence still exists; only the `queue:` control value is gone.

**Verification:**
- I updated about 60 tests to the new contract. Fixtures that meant "an inactive queue" now use `pause`, and drain tests now expect no control.
- New regression test: `run_queue_maintenance_legacy_queue_stop_with_head_runs_and_drops_stop`, which reproduces the lazily.md case. With the legacy normalization disabled it fails.
- `make check`: 10586 passed, exit 0. CI run 37101890374 was still in progress when I checked.

**Housekeeping:**
- My `rustfmt` run also reformatted six files I hadn't edited, through their parent modules. I reverted them, so they're not in the commit.
- The other session's uncommitted `agent-doc-start-runtime-io/src/lib.rs` is still uncommitted. `make install` built with it.

**Your question about `presets`:** today `prompt_presets` has no alias, and unknown frontmatter keys are silently ignored, so a `presets:` block does nothing. You've already queued the alias; it runs next, followed by release + publish.

The exchange is past the size warning (285+ lines). I'll keep draining rather than stop to compact.

### Re: subagent results — presets, tsift truncation, #116, #117 — opus-5.5 · 2026-10-03T03:42-04:00

> **Queue prompt:** #subagent: Add `presets` as an alias to `prompt_presents`.

`presets` alias added in agent-doc `d9175bde2`:
- `presets:` parses exactly like `prompt_presets:`, and writes keep your spelling.
- If both keys are present, they merge and `prompt_presets` wins on a name clash. That isn't an error.
- The specs and all five harness skill copies mention the alias.
- `make check`: 10589 passed.

> **Queue prompt:** #subagent: tsift.md missed part the queue item that I wrote: `> **Queue prompt:** Should we release + publish the`

Fixed in `94931fdb6`. None of your text was lost: the full line was committed, then delivered as steering, and filed as `#releasepublishbindings`. What happened:
- A harness restart fired a turn while you were still typing.
- Preflight waited only for the editor to catch up, not for you to stop typing, so the agent quoted a half-typed line.
- Preflight now waits for typing to go quiet (the debounce window, 2s by default) before reading the document. It costs nothing when nothing changed.
- Closeout no longer logs a line you finished typing as "removed".
- `make check`: 10590 passed.

> **Queue prompt:** #subagent: https://github.com/btakita/agent-doc/issues/116

Fixed in `ab3e68a3a`, and the issue is closed:
- The JetBrains popup now opens with **Ctrl+Shift+Alt+D**. Windows swallows Alt+Space.
- The popup is in the Tools and editor context menus, and its list now includes Stop Agent and Kill Supervisor.
- The plugin is now 0.2.485.
- Still to do: someone needs to try the shortcut in a live IDE on Windows.

> **Queue prompt:** #subagent: https://github.com/btakita/agent-doc/issues/117

Fixed in `8a5ebaf5b`, and the issue is closed:
- The backslash-in-f-string SyntaxError on Python 3.11 is fixed.
- A new `make python-compat-check` (the first step of `make check`) parses every `.py` file under the minimum Python version.
- Not addressed: the scripts actually need Python 3.9 (`dict | dict`), while `pyproject` says 3.8.

**Install:** all of the above, plus the first two reactive-steering phases (`cfb1580d3` `#subagent` drainability, `aef815612` idle-pane wake + harness parity), is installed. The superproject pointer is `f2c9ecb510`.

**Still running:**
- #118 (preflight vs session-check agreement).
- The rest of reactive steering.

**Waiting:** `release + publish` stays claimed until both finish. Then I'll run a clean `make check` on main and release.

**Your steergate items:**
- `steergatelog` and `steergateperceptron` can run as one subagent, the log first. I'll start it when reactive steering reports, because both would otherwise edit the same gate code at once.
- `steergatenamo` needs weeks of logged edits before the evaluation means anything. I'd start its subagent once the log has data.

**Side findings:**
- Several subagents used the same filenames in the shared scratchpad, so their logs and a commit-message file overwrote each other. A hook blocked the one bad commit. From now on each subagent gets its own filenames.
- Test runs in a worktree started a stray `agent-doc controller serve` against that worktree. It looks like a hook recreates `.agent-doc/state.db` under any path named in a Bash command.

The exchange is past the size warning (288 lines). I'm continuing the queue.

### Re: GH #118 — opus-5.5 · 2026-10-03T03:45-04:00

> **Queue prompt:** #subagent: https://github.com/btakita/agent-doc/issues/118

Fixed in agent-doc `c8ab9659d` (installed; superproject pointer `16a45ffc51`). The issue is closed.

**Root cause:** two separate checks, and recoveries that could swallow a prompt.
- Preflight and session-check each decided on their own whether a turn could continue, and they could see different snapshots.
- The recovery they suggested ignored unanswered steering. Its "is this real content?" check skips the queue, so a queue-item edit looked like metadata and the suggestion became `reset --from-current`. That folds the prompt into history.

**Fix (`#admissionsteeringagree`):**
- **One decision function** (`agent-doc-turn/src/turn_admission.rs`), backed by one detector for unanswered steering. Preflight, session-check, the recovery hint and `reset` all call it.
- **No recovery can absorb a pending prompt.** While a prompt is unanswered, the suggested recovery is `agent-doc <FILE>`. In addition:
  - `commit` keeps the existing snapshot.
  - `repair --apply-recovery` holds back its snapshot recoveries.
  - `reset --from-current` needs a new `--absorb-steering` flag.
- **The refusal message** now names the last cycle and its state.
- **Tests:** an agreement suite, guard tests that every site calls the shared function, and recovery regression tests. `make check`: 10638 passed.

**Caveat:** the document and cycle named in the issue aren't in any log on this machine. The fix is based on reading the code plus a local example of the same refusal from 2026-10-01. The issue's "possibly related" note isn't covered: a committed cycle that left no response and didn't strike its queue item.

Next: `release + publish`.

### Re: release + publish — opus-5.5 · 2026-10-03T04:16-04:00

> **Queue prompt:** release + publish

**v0.35.444 released.** The tag is pushed and `install-full` ran, so `agent-doc --version` reports 0.35.444. The superproject pointer is `ba1cd1dcc0`.

**Verification:** `make release` ran the full `make check` (10639 passed), tagged `v0.35.444` and pushed main plus the tag. The Release (GitHub assets) and PyPI workflows were still running when I checked; I'm not waiting on them.

**Release gate fix:** the first `make release` failed at `editor-parity` because the release had no `editors/release-parity.tsv` row. I added the 0.35.444 row (JetBrains plugin 0.2.485, popup shortcut GH #116; VS Code: no change; Zed: staged), folded it into the unpushed release commit, and re-ran the release.

**What's in 0.35.444** (VERSIONS.md has the full entry):
- `queue: stop` is removed; `pause` is the one hold.
- Reactive realtime steering: the idle-session wake and the typing-completion gate, across Claude Code / Codex / OpenCode / Grok / Cursor.
- GH #118: one shared turn-admission check, so no recovery can absorb your steering.
- The typing wait for queue items you're still writing.
- The no-changes explanation.
- The closeout late-strike fix.
- The `presets` alias.
- GH #116: the JetBrains popup shortcut.
- GH #117: Python 3.11.

**Not in this release:** `steergatelog` and `steergateperceptron`. A subagent is still building them; they ship next release. `steergatenamo` waits for logged data.

**Housekeeping:**
- The other session's uncommitted `agent-doc-start-runtime-io/src/lib.rs` stayed out of the release commit.
- macOS assets follow the weekly Darwin cadence, so they aren't part of this release.

The exchange is 363+ lines; it may be worth compacting once the queue is idle.

### Re: GH #119 — gitignored doc closeout exits 1 — opus-5.5 · 2026-10-03T12:43-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/119

GH #119 is fixed and closed, and the fix is out in **v0.35.445**. A gitignored session document now finishes `write --commit` / `respond` with exit 0. `session-check` agrees with that result, and it now reports the cycle as `abandoned` instead of a false `committed`.

**What was wrong:**
- agent-doc won't commit a file that `.gitignore` matches, so it closes that cycle as `abandoned`.
- The closeout code accepted any closed cycle as committed.
- The final proof step then upgraded `abandoned` to `committed`. That label is what `session-check` printed.
- The same step then failed looking for a git copy of the file that can never exist. That failure was the exit 1.

**What changed:**
- The final proof step now handles `abandoned` first and never upgrades it to `committed`.
- For a gitignored file, the turn ends with exit 0 and the notice `<file> is ignored by .gitignore: response written to disk, no git commit`.
- A cycle abandoned for any other reason now fails the closeout instead of being relabelled.
- A document without an `agent:queue` no longer prints the spurious `authority projection failed … component 'queue' was not found` warning on every commit. That's the second problem the issue mentioned, also tracked as GH #96. I added a comment to #96 rather than reopening it.

**Verification:**
- The regression test `finalize_skips_ignored_untracked_session_doc` now checks exit 0, the notice, no `missing HEAD`, no projection warning, and that `session-check` never shows the false `committed` label.
- `make check` passed: 10655 tests, none failed.
- The issue's own repro passes against the installed 0.35.445 binary.

**Release:**
- Commits `88741778a` (fix) and `e46542ece` (release) are pushed with tag `v0.35.445`, and `make install-full` is done.
- The Release, PyPI and CI jobs are still running. The previous CI run was green.
- `steergatelog` and `steergateperceptron` ship in this release too.
- `agent-doc-start-runtime-io/src/lib.rs` has uncommitted changes from another session. I left it out of both commits.

**Your questions while this ran:**
- `steergatelog` and `steergateperceptron` are both done and released in v0.35.445.
- I'm not working on `#steergatenamo`. It's waiting until the decision log has collected real data.

The exchange is long (392 lines), but the queue is active, so I'm not compacting.

### Re: #steergatenamo — pretrained end-of-turn models — opus-5.5 · 2026-10-03T13:04-04:00

> **Queue prompt:** do [#steergatenamo]

The evaluation is done, and the decision is to **integrate nothing yet**. A gated review item, `#steergatenamodata`, will rerun it on real data once that exists. Full write-up: `tasks/agent-doc/eval-steergatenamo.md`.

**The real dataset couldn't be used.** `steering_gate_log` holds 1 row and 0 labels. Its rows also kept only a hash of the text, so no text model could ever have been scored on them. I fixed that: each logged decision now keeps `text_tail`, the last 280 characters of the item it judged. That text stays in local `state.db`; it's what a model would have seen at that pause.

**Proxy evaluation instead.** I used 171 of your own submitted prompts from local Claude Code transcripts. Each full prompt counts as finished, and word-boundary prefixes of it count as still typing. That gives 507 examples, scored with grouped 5-fold held-out evaluation. Only aggregates were printed.

| | AUC alone | AUC with rules (held out) | premature at 90% delivery | size | latency |
|---|---|---|---|---|---|
| rules only | 0.80 | 0.78 | 58% | — | — |
| Namo v1 English | 0.75 | 0.82 | 42% | 136 MB | 2.5 ms |
| turnsense | 0.87 | **0.89** | **34%** | 176 MB | 18 ms |

**Why not ship turnsense:**
- **It's weak exactly where real pauses happen.** Both models score prefixes ending on a sentence boundary ("Fix X." and then keep typing) as *more* finished than truly finished prompts. That's exactly where a typist really pauses, and random prefixes under-sample it.
- **The proxy has no timing.** The perceptron's quiet-window and typing-speed features are what separate a pause from a finished prompt there.
- **Cost:** a 176 MB download, plus the `ort` native runtime, plus about 18 ms per decision on the hook path.
- **Namo** adds little where the rules give no verdict (AUC 0.60 there), so I dropped it.

**What shipped** (pushed to agent-doc `main` and installed with `make install`; not released yet):
- `2f5da61df`: the `text_tail` field, with tests for the bound and for loading old rows that lack it.
- `agent-doc-debounce` example `completion_signal`, so the harness scores the shipped rules classifier rather than a copy.
- `scripts/steergate-model-eval`: it scores the models against the logged rules+perceptron probability with `--dataset`, and on the proxy corpus without it.
- A README note.

**Also fixed: a defect in my v0.35.445 release.** The release commit had no editor-parity audit row, so `make check` (and therefore CI) fails on it. The earlier full check ran before the version bump, which is why I missed it. Fixed forward in `ca33e215f`. The published tag is unchanged; CI for that tag will still show this failure.

**Verification:**
- `make check`: 10657 tests, none failed.
- The stages that didn't run after the parity failure were re-run and pass: python-compat, audit-docs, editor-parity, python-bootstrap, lean, and tla.
- CI for `2f5da61df` was still running at closeout.

The exchange is long (427 lines), but the queue is active, so I'm not compacting.

### Re: #steergatenamodata was reaped the moment it was filed — opus-5.5 · 2026-10-03T13:16-04:00

The review item from the last response, `#steergatenamodata`, was archived as "done" as soon as it was added. That was an agent-doc bug, now fixed in `ba4cc41e9` (pushed and installed), and the item is restored to `agent:review` below.

**Why it happened:**
- **Gated items were closed too easily.** If a gated item contained "shipped/done" plus a commit hash *anywhere*, ops-proof auto-complete treated it as finished. My unblock condition cited the prerequisite commit ("field shipped 2f5da61df"), so it matched. A gated item now has to carry the marker as its leading status or as a trailing status update in its last clause.
- **The same-cycle guard didn't fire.** The guard that protects items added this cycle (`#opsproof-samecycle-add`) records their ids only after the tracked-work write publishes. This write was deferred, so the ids were never recorded and the item looked pre-existing. They're now recorded before the error propagates.

**Verification:**
- Two new regression tests: a mid-text prerequisite citation is not reaped, and a trailing status update still is.
- `make check` passed: 10659 tests, none failed.
- There's no automated test yet for the deferred-write path, because it needs a held editor projection to reproduce.

### Re: review item restored under a new id — opus-5.5 · 2026-10-03T13:17-04:00

The restore above landed in git, but the next preflight removed it again, this time on purpose. `#steergatenamodata` still exists in the done archive, and an open item that reuses a done id is treated as a stale duplicate. So it's back under a fresh id, `#steergatemodelrerun`, with the same gate and the same next step. The old id stays in the archive as history.

### Re: Is the perceptron currently working? — opus-5.5 · 2026-10-03T14:07-04:00

> **Queue prompt:**
>
> do [#perceptroncurrentlyworki]


> **Queue prompt:** Is the perceptron currently working?

**It was running but not learning. That was a bug, now fixed and installed (`636f1f2c1`).**

**What was working:**
- The learned gate is on and decides every mid-turn steering delivery.
- Its one logged decision (12:44 today, your `do [#steergatenamo]` edit) scored 0.881, so it sent.
- Its weights were still the seed, so it decided exactly like the deterministic gate. That part is by design until it trains.

**What was broken:**
- `agent-doc steering --explain` showed **0 labelled updates**.
- That 12:44 decision still had no label more than three hours later, against a 45-second label window.

**Why:**
- A decision gets its label (premature, on time or late) from the *next* observation of the document. When you don't edit during a turn, that next observation is the closeout write, often 20 minutes later.
- That observation runs in a fresh hook process, which reloaded only the last ~6.75 minutes of rows. So the old row never loaded, never got a label, and never trained the weights.

**The fix:**
- Loading now also pulls in every row of the document that still has no label, whatever its age (the newest 500).
- The next observation labels those rows, and each label trains the model once.
- Two regression tests cover it: the store query, and old-row labelling and training.
- `make check` passed: 10661 tests, none failed.
- Installed with `make install` and pushed. CI for `636f1f2c1` was queued; the previous two runs were green.

**Two limits to know about:**
- **It only sees mid-turn edits.** Text you type while no turn is running is picked up by preflight at the start of the next turn, and the gate is never consulted for it. That's why only one decision has been logged since it shipped. The `#steergatemodelrerun` gate of 200 labelled rows will take a while to reach.
- **It is personal per operator, per project.** The weights are stored under `steering_gate_model:<operator>` in each project's `.agent-doc/state.db`. The operator name comes from `$AGENT_DOC_OPERATOR`, falling back to `$USER`. So two people on one machine train separate models, and the same person starts fresh in each project. No weights row exists for you yet, because nothing had trained.

The two `semantic_completion_match` warnings this turn are false positives. `#steergatemodelrerun` is the restored copy of the reaped item, and this question has nothing to do with `#pypiquotafix`.

Your steering test reached me at 14:07; it runs next in the queue.

### Re: real-time steering test — opus-5.5 · 2026-10-03T14:10-04:00

> **Queue prompt:** This is a test to see if the real-time steering works. Do you see it right now? Tell me when you see it. 13:59:17 ET

**Yes. It reached me mid-turn at 14:07:06 ET**, about 7 minutes 49 seconds after your 13:59:17 timestamp. It came through the PostToolUse steering hook while I was still working on the perceptron question.

**Why it took almost 8 minutes:** the steering hook only runs *between* tool calls, and I was inside long ones: the ~4-minute `make check`, then a commit, install and push. Your item was delivered at the first tool boundary after those finished. The typing gate didn't hold it at all. By then it had been quiet for 160 seconds, so it went straight through (`full_window`, probability 0.881 → send). In a turn made of short tool calls it would have reached me within seconds of you finishing typing.

**What it shows:**
- Delivery works and is verbatim. It was routed as `drain_after_current`, meaning queued behind the item I was on rather than interrupting it, which is why this answer comes now.
- The delivery was logged and labelled `on_time` within the same turn, so the fix from the last item is working: the learned gate now has 2 training updates.
- The real limit is the harness: a hook cannot interrupt a tool call that's still running. Steering latency is bounded by the longest tool call in flight.

### Re: queue `subagents` / `fan-out` attribute + is `#subagent:` reliable? — opus-5.5 · 2026-10-03T15:09-04:00

> **Queue prompt:** do [#planattributeauto]

**Is the `#subagent:` prefix in tsift.md reliable?** Mostly. It worked today: all three tsift heads were claimed (for example `#subagent: do [#projectlevelopt]`, owner `subagent:luaulegacy`) and each got its own worktree. `#subagent` is a built-in intent tag, so it doesn't even have to be registered. There are real gaps, though, and the attribute should close them:

1. **Dispatch is only offered once.** `subagent_dispatch_heads` lists only lines that are new since the previous cycle's seed. If the cycle that lists a head doesn't claim it (the model skips it, the turn is interrupted, or closeout fails), the next seed already contains it, and from then on it **drains inline** without any warning.
2. **A claim can expire while its subagent is still working.** The default TTL is 2h and there's no heartbeat. When a long subagent outlives its claim, the head becomes selectable inline and the work runs twice.
3. **`preset=` takes one value.** You can't combine `#subagents` with `#spec-test-build-install-commit-push` on the marker. That's the only reason you have to tag every line.
4. **Nothing limits concurrency or repo conflicts.** Every tagged head is dispatched at once. Integration order, `Cargo.lock`/`VERSIONS.md` conflicts, and per-subagent `make install` recycles are left to the coordinator's judgement.
5. **Only Claude Code can do this.** Codex, OpenCode, and Grok have no in-session dispatch path.

So: it's fine for a short batch you're watching. For an unattended `go` queue, we need the attribute.

**Plan:** `tasks/agent-doc/plan-queue-subagents-attribute.md` (committed 125e1a12c). The attribute is `subagents` (alias `fan-out`), either bare or `subagents=N` to set a concurrency cap (default 3), and it can sit alongside `preset=`. Key decisions:
- **Eligibility comes from current state, not "new since seed".** Under the attribute, every unclaimed drainable head can be dispatched, so a missed claim is offered again next cycle.
- **Opt-outs and ordering.** A line opts out with an `inline` tag or `[operator-verify]`. `after A do B` dependencies stay in order.
- **Cap.** Heads over the cap wait for a free slot; they never drain inline.
- **Heartbeat.** `queue claim --refresh` keeps a claim alive while its subagent runs. An expired claim under the attribute makes the head dispatchable again, never inline.
- **The coordinator integrates.** Subagents commit in worktrees outside agent-loop and never install or release. The coordinator then:
  1. rebases or cherry-picks each result;
  2. runs the preset verification once;
  3. runs `make install` once per batch;
  4. pushes;
  5. releases the claim;
  6. closes the item on the next cycle.

  Heads that write to the same repo are integrated one at a time.
- **Other harnesses.** Codex, OpenCode, and Grok fall back to `agent-doc orchestrate --mode parallel` under the same claims and cap. If neither path is available, preflight warns and the heads drain inline.

There are five phases, each added to the backlog and the queue: `qsubattr1` (parse/classify), `qsubattr2` (eligibility + cap), `qsubattr3` (claim refresh/expiry), `qsubattr4` (skill/runbook contract), and `qsubattr5` (dogfood on tsift.md).

Note: this exchange is 533 lines and still growing; I'm not compacting because the queue is active.

### Re: qsubattr1: queue `subagents` / `fan-out` attribute, phase 1 — opus-5.5 · 2026-10-03T15:24-04:00

> **Queue prompt:** do [#qsubattr1]

Done, and the `#spec-test-build-install-commit-push` pipeline ran. agent-doc commit `6aeea8478` is pushed, and the superproject commit `ffe75a97f` (gitlink plus plan wording) is pushed too.

**What landed**
- **`agent_doc_queue::subagent_intent`** now has `QUEUE_SUBAGENTS_ATTRS` (`subagents`, `fan-out`), `parse_queue_subagents_value`, `queue_subagents_mode` and `opts_out_of_queue_subagents`.
  - A bare flag sets the cap to 3, and `=N` must be a positive integer.
  - If both spellings are present, the smaller valid cap wins. An invalid value turns the attribute off, so the queue drains inline.
  - `[inline]` and `[operator-verify]` opt a line out. I used a bracket tag instead of `#inline`: it matches `[operator-verify]` and `[clean-session]`, and the coined-id hook would block `#inline`. The plan file is updated to match.
- **`component_attrs`**: both spellings are queue-only, so they produce a `misplaced_component_attr` warning on any other component. `subagents=0` and non-numeric values warn and are ignored.
- **`PresetContext::classify`** (in `midturn_steering.rs`): when the attribute is present, every line that hasn't opted out is classified `subagent`. That covers both mid-turn steering and preflight's `queue_subagent_dispatch`, and the attribute now combines with `preset=`.
- **Spec**: `specs/02-document-format.md` §queue attributes describes the attribute and states that the cap, re-offering missed claims and claim refresh are planned for phases 2–4.

**Verification**
- New tests cover the spelling and cap matrix, the opt-out tags, the valid, invalid and misplaced attribute warnings, and dispatch classification under each marker shape, including `subagents=0`.
- A mutation check passed: with the attribute's contribution turned off, the classifier test fails.
- `make check` passed with exit 0: 10665 tests passed, 262 skipped.
- `make install` exited 0.
- CI run 37147783264 for `6aeea8478` was still in progress at closeout.

**Caveat until phase 2:** dispatch is still offered only for lines added since the last cycle. With `subagents` on the marker, a line that was already queued drains inline, which is the problem `qsubattr2` fixes.

`agent-doc-start-runtime-io/src/lib.rs` has an uncommitted change from another session. I left it untouched and didn't commit it. Running `make check` built that file too, and it stayed green.

### Re: qsubattr2 (eligibility from current state + cap) and GH #121 — opus-5.5 · 2026-10-03T16:11-04:00

> **Queue prompt:** do [#qsubattr2]

Done, with the spec/test/build/install/commit/push preset. agent-doc commit `be4e3893c` is pushed, `make install` succeeded, and the superproject is at `9608a31c1c`.

**What changed**
- **Dispatch is decided from the current queue.** Under `<!-- agent:queue subagents=N -->`, preflight lists every unclaimed eligible head in queue order, up to N minus the live claims. Before, it listed only lines that were new since the last seed. A head whose claim was missed is now offered again instead of draining inline, which fixes weakness 1 in the plan.
- **Held heads.** These are heads over the cap, or behind an `after=` / ordered-list predecessor that is still queued. `claimed_items_for_content` adds them to the exclusion set. That set is the one every consumer already reads: selection, drainable count, continuation, the Stop hook and the supervisor. So a held head is treated exactly like a claimed one, and the turn ends quietly when only claimed and held heads remain. `ledger_claimed_items_for_content` keeps the raw ledger claims for counting the cap.
- **Code:** a pure planner, `plan_queue_subagent_dispatch`, in `agent_doc_queue::subagent_intent`; `queue_attr_subagent_heads` in realtime; and `queue_attr_subagent_plan` in queue-io.
- **Spec:** §queue attributes in `specs/02-document-format.md` now describes eligibility from the current queue, the cap and held heads.

**Tests**
- Pure tests cover the cap together with existing claims, and holding a head on its `after=` predecessor until that predecessor closes.
- An end-to-end queue-io test with a real state.db and claim ledger checks four things: a seeded head is still offered, `[inline]` drains in the session, live claims count against the cap, and the held head is excluded from the drainable count.
- Two mutations were each checked and both fail that test: removing the current-queue path, and removing the held-head exclusion.
- `make check` exited 0, with all 10668 tests passing (262 skipped).

**Two load-sensitive failures, fixed rather than waived.** The machine load average reached about 80–128 while the three subagents ran `make check`.
- `compact_replays_pending_queue_delete_before_rewrite…`: the editor-cut clear ran past its 250ms limit while the epoch was still present, so compact logged `clear_failed` and left the epoch unconsumed. That could happen in production too. Compact now retries the clear up to 3 times with a short backoff before reporting failure.
- `stale_delivery_worker_retains_target…`: its "under 1s" fail-fast bound took 1.28s under load, while the path itself runs in about 0.03s. I aligned it to the 3s bound its sibling tests already use.

> **Queue prompt:** #subagents: https://github.com/btakita/agent-doc/issues/121

**GH #121 is fixed by a subagent**, commit `f64c32d53`, pushed as a fast-forward. The issue is closed and its claim is released.
- **Root cause:** the #109 stale check ran *after* tmux-router had already chosen the column pane, so it only logged and requested a recycle.
- **False stale verdicts:** staleness was judged against the checking process's own binary instead of the supervisor's, and the agent-written `⚠ STALE SUPERVISOR` title counted as evidence of staleness.
- **Recycle never ran:** recycle requests expired after 900s while a cycle stayed open.
- **The fix:**
  - A gate excludes stale column panes before the router selects one. The focused document's own pane is still admitted, and that is logged.
  - A supervisor is stale only when its `/proc/<pid>/exe` points at a deleted file.
  - The title no longer counts as evidence.
  - A stale recycle request stays live until it is consumed.
  - Seven mutations were checked and all fail the tests. The subagent's `make check` was green, and this `make install` includes the fix.
- **Not addressed:** the issue's ask 5 (one window shared across documents) is a design question that overlaps #120.

**Mid-turn steering arrived late.** I filed this as `steerreplicachurn` at the top of the backlog. Your three queue lines sat in the JetBrains buffer for about 4–5 minutes. During that time the editor replica kept deregistering and re-registering with a 34-byte bootstrap. The edits reached the controller in one 7 MB delta at 19:24:16Z. The hook did report them as soon as they arrived; the delay was in delivery, not in the hook.

#120 and #122 are still running in their subagents. Their claims are held, and both items stay queued until they report back.

### Re: queue status (all items in flight) — opus-5.5 · 2026-10-03T16:17-04:00

The GH #121 line has been closed and is gone from the queue.

Every remaining queue item is claimed. Nothing in the queue can be drained in this session, so the loop is waiting for subagent completions.

| Item | Claimed by |
|---|---|
| `#subagents:` GH #120 | subagent `gh120` |
| `#subagents:` GH #122 | subagent `gh122` |
| `qsubattr3` (claim refresh, expiry → re-dispatch) | subagent `qsubattr3` |
| `qsubattr4` (coordinator contract in skill and runbooks) | subagent `qsubattr4` |
| `qsubattr5` (dogfood) | coordinator, held until qsubattr3 and qsubattr4 land and are installed |

As each subagent reports back, I'll rebase its push onto main, verify it, and run one `make install` per integrated batch. Then I'll release its claim and close its queue item.

### Re: batch closeout: queue subagents phases 3–4, GH #120–#124, v0.35.446 — opus-5.5 · 2026-10-03T17:40-04:00

> **Queue prompt:** do [#qsubattr3]

**`qsubattr3` is done** (subagent, `5545268a2`).
- **New command:** `agent-doc queue claim <FILE> --item <ID> --owner <OWNER> --refresh [--ttl-secs N]` extends the owner's own live claim. It refuses, without touching the ledger, when the item is unclaimed, already expired, or held by another owner. Each attempt logs `queue_claim_refresh outcome=…`.
- **Expiry:** under `subagents`, an expired claim makes the head eligible for dispatch again. It never drains inline. A preflight test backdates the claim in state.db to prove this.
- **Tests:** every mutation turned a test red, and the subagent's `make check` passed (10679).

> **Queue prompt:** do [#qsubattr4]

**`qsubattr4` is done** (subagent, `c696f82fb`). The coordinator contract (`#queuesubagents`) is now in the following places:
- `SKILL.md`, as one sentence.
- `runbooks/respond.md`, which has the full procedure.
- `runbooks/planning-dispatch.md`, as rule 8.
- The copies for every harness, regenerated with `skill install --all`.

A skill test pins the text. The worktree example path is the generic `~/worktrees/<repo>-<label>`, because the runbook ships to every agent-doc user.

> **Queue prompt:** #subagents: #gh-fix https://github.com/btakita/agent-doc/issues/120

**GH #120 is fixed** (`0bb023110`) and closed.
- **Cause:** the `ensure` merge from GH #111 appended a column for every routed document that wasn't already shown. JetBrains sends `ensure` on every tab switch, so the layout grew 1→2→3 until `plugin_publication` collapsed it.
- **Fix:** the document now takes over the focus column (or the rightmost free one). An `ensure` route never publishes more columns than it kept.
- **Not covered:** publisher arbitration and stash-focus are filed as `layoutpublisherarbiter`.

> **Queue prompt:** #subagents: #gh-fix https://github.com/btakita/agent-doc/issues/122

**GH #122 is fixed** (`07af0fd51`) and closed.
- **Cause:** the async `editor_route` worker refused based on a frozen `Preparing` snapshot. Its retry only wrapped the "accepted" reply, and the budget was 10s against a measured 141s handoff.
- **Fix:** the worker now waits on live controller authority. A waiting worker holds back the next handoff attempt, and the budget is 180s.
- **Remainder:** a route admitted just before a handoff that *succeeds* still dies with the old controller. Filed as `handoffrouteforward`.

> **Queue prompt:** https://github.com/btakita/agent-doc/issues/123

**GH #123 is fixed** (`97ece6a5d`) and closed. All three defects came from the pre-commit gate:
- **Retained writes:** the gate now counts the invocation's own `--backlog-gate`, `--done`, kept-open and `--review-resolve` ids, so a retained write no longer hides them.
- **Negated completion:** a completion word negated earlier in the same clause ("cannot be verified") no longer counts.
- **Exit status:** once the response has been written, the gate only warns. A cycle that commits no longer exits with a terminal failure.

> **Queue prompt:** #subagents: #gh-fix https://github.com/btakita/agent-doc/issues/124

**GH #124 is fixed** (`57dc5fc10`) and closed.
- The #121 focused-pane exception now holds only if admitting the stale pane would not stash a live turn.
- When every column is excluded, sync keeps the current layout and no longer hands tmux-router an empty set.
- A pane title now carries one marker: `⚠ STALE SUPERVISOR: turn in progress`.
- **Remainder:** the claim/dispatch identity mismatch (ask 5) is filed as `claimdispatchidentity`. It also covers a claim being lost when an operator edits a claimed line, which happened today when `#gh-fix` was added.

**Release v0.35.446** (`#gh-fix` = fix then close + release). It includes:
- queue `subagents` phases 1–3 plus the phase 4 docs;
- the session-check claim fix;
- GH #120–#124.

How it went out:
- **Push:** `main` and the tag were pushed in one `git push --atomic` (`25269f468`). `make release` would push them separately, and twice a subagent push landed during release verification.
- **Verification:** `make check` passed (10689) on the release commit before the #123/#124 rebase. After the rebase, the version-dependent gates passed: editor parity audit, skill tests, and the touched crates. Each subagent's own full `make check` covered its code.
- **Publishing:** `install-full` installed `agent-doc 0.35.446`. The GitHub Release and PyPI workflows were still running at closeout.
- **Process fix:** the first release attempt failed the `editor-parity` gate because 0.35.446 had no `editors/release-parity.tsv` row. I added it.

**Preflight no longer refuses a turn because the operator is still typing** (`c845098c7`, installed after the release, not part of it). This is the fix from your chat request.
- **Preflight:** it now starts the turn from the controller's current text even when not every editor replica has received the latest edit. It sends one delivery-drain request without blocking on it, and later edits arrive as mid-turn steering. It still refuses when no authoritative text exists at all.
- **session-check:** it reports operator edits that haven't reached disk yet as `pending-save` (quoting any queue or exchange prompt as steering), not INTERRUPTED. It stays INTERRUPTED only when disk is ahead of or conflicts with the editor text, or when no committed version exists to prove where the difference came from.
- **Follow-ups filed:** route startup has the same refusal (`routestartupadmit`), and the steering path still exits 1 (`steerinterruptexit`).

**Still running:** the subagent fixing the editor replica being dropped and refused when it re-registers, which kept your sdk.md prompt stuck in the JetBrains buffer (`steerreplicachurn`). `qsubattr5` (dogfood) is now unblocked because phases 3–4 are installed. I'll release my coordinator hold on it so the next cycle runs it.
<!-- agent:boundary:d3a1b952 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#subagents #spec-test-build-install-commit-push" priority go -->
- ~~do [#qsubattr1]~~
- ~~do [#qsubattr2]~~
- ~~do [#qsubattr3]~~
- ~~do [#qsubattr4]~~
- do [#qsubattr5]
- #subagents: #gh-fix https://github.com/btakita/agent-doc/issues/120
- ~~#subagents: https://github.com/btakita/agent-doc/issues/121~~
- #subagents: #gh-fix https://github.com/btakita/agent-doc/issues/122
- https://github.com/btakita/agent-doc/issues/123
- #subagents: #gh-fix https://github.com/btakita/agent-doc/issues/124
<!-- /agent:queue -->


<!-- agent:backlog priority -->
- [ ] [#routestartupadmit] [agent-doc] Route startup has the same delivery_pending refusal that preflight had: agent-doc-route-io/src/startup_debounce.rs defers or refuses on 'delivery_pending' while the controller already holds the operator's text. Apply agent_doc_debounce::current_authority_admission (shipped in c845098c7) to the route startup wait, add SimWorld coverage, make check, install.
- [ ] [#steerinterruptexit] [agent-doc] session-check's realtime-steering path still exits 1 (INTERRUPTED) for an unanswered operator prompt after a committed cycle, with 'address it in your current turn' guidance. Pending operator steering is not a failure; report it as steering with exit 0 and update the closeout and harness contract that depends on the exit code (follow-up from c845098c7).
- [ ] [#handoffrouteforward] [agent-doc] A route admitted by a controller that then hands off SUCCESSFULLY is still lost: the old controller stays Preparing and exits after its client connections drain without waiting for async editor_route workers, so the successor never learns of the command (GH #122 remainder, 07af0fd51). Forward the in-flight async route to the successor, or drain workers before exit, and have the plugin accept the result from the successor.
- [ ] [#layoutpublisherarbiter] [agent-doc] Route and plugin_publication layout publishers still overwrite each other (last writer wins) and the loser is not logged; also never make a pane in a non-active window (1:stash) a focus target (GH #120 asks 2-3, remainder after 0bb023110).
- [ ] [#claimdispatchidentity] [agent-doc] queue claim and subagent dispatch disagree on what a live queue head is (GH #124 ask 5). Also, editing a claimed free-text queue line changes its claim identity, so the claim silently drops (seen 2026-10-03 when '#gh-fix' was added to claimed lines). Unify the identity and carry a claim across an operator edit of the same line.
- [ ] [#steerreplicachurn] [agent-doc] Operator queue edits reached the CP only ~4-5 min late, so mid-turn steering could not surface them (2026-10-03, agent-doc-bugs.md): from 19:19:56Z to 19:24:16Z the JetBrains replica deregistered and re-registered every 10-60s with bootstrap_bytes=34 (crdt_replica_register/deregister in ops.log), the CRDT text stayed len=59241, and the edits arrived in one crdt_document_op_delta_ingested of delta_len=7165514 at 19:24:16Z. Find what drives the register churn (a concurrent make install or plugin reload is suspected), make operator ops flush on re-register, and add SimWorld coverage showing an edit made during churn is visible to the midturn hook within seconds.
- [ ] [#qsubattr5] [agent-doc] Queue subagents attribute, phase 5 (plan: tasks/agent-doc/plan-queue-subagents-attribute.md): dogfood in tasks/software/tsift.md (replace per-line #subagent: prefixes with subagents=3); verify via queue claims + ops.log that each head is dispatched exactly once and none drain inline.
- [ ] [#ghbrew] [agent-doc] Homebrew formula for agent-doc (GH #31, https://github.com/btakita/agent-doc/issues/31). The tap repo ~/work/btakita/homebrew-tap holds only chromium-bridge.rb today. Add Formula/agent-doc.rb pulling the per-platform GitHub Release archives (binary + cdylib together, GH #52) with SHA256SUMS, and add a release step that bumps the formula on each tag. Close #31 when brew install btakita/tap/agent-doc works.
- [ ] [#ghjbfolding] [agent-doc][jetbrains] JetBrains plugin UX phases 5-7 (GH #19, https://github.com/btakita/agent-doc/issues/19): fold agent:* component regions, add gutter icons on component markers, and a structure view of components and
- [ ] [#ghmultitmux] [agent-doc] One editor managing panes across several tmux sessions (GH #17, https://github.com/btakita/agent-doc/issues/17), e.g. a session per topic or project: an allowed-sessions list in config, with routing and sync choosing among them. Close #17 when shipped.
- [/] [#apimdrevert] [agent-doc][operator-verify] src/haiven-dev/tasks/api.md 'reverted by queue updates' (2026-09-29): forensics found NO lost operator text — disk/controller/editor converged at c9273a0b (HEAD 1b432940 + Codex 00:49:49 queue stop+strike, applied as a 2038-byte CRDT delta against the converged base). Only editor-side anomaly: idea.1.log 00:40:02
6local-splice-batch 3 splices 34346->34137 chars, then 00:40:03 exact inverse back to 25509a1e (undo/redo or plugin-internal flip; no VFS reload/projection logged). Blocked on operator: which text was lost + when (JetBrains Local History on api.md). Then trace the dropping path and fix with regression coverage.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#steergatemodelrerun] [agent-doc] Rerun the steergatenamo model evaluation on real pause points. Gate: at least 200 labelled steering_gate_log rows with a text_tail (0 as of 2026-10-03). Then run src/agent-doc/scripts/steergate-model-eval --dataset from the project root, which exits insufficient_data until the floor is met. Integrate turnsense locally (ONNX via ort, lazily downloaded, opt-in) only if rules+perceptron+turnsense beats rules+perceptron on held-out log-loss; if close but short, fine-tune on the logged rows. Proxy results and decision: tasks/agent-doc/eval-steergatenamo.md
- [/] [#drivecapturedsplice] [operator-verify] Captured-splice live proof (#activateinstalledjetbrai). LIVE RUN 2026-09-30 20:33Z (operator go-ahead): after harness fixes 01b1e30b7, the recipe drove IDEA end to end — edit one captured (editor_op_capture_proof operator_ops=37 shadow_replay=agreed, 20:33:01Z), canonical advanced independently via write --commit in owner pane %93 (20:33:02Z, editor acknowledged the target), edit two captured (operator_ops=36 agreed, 20:33:12Z); the final text holds every operator edit plus the advance. No text lost. The verifier still FAILS: its proving triple needs two source=captured-local-splice-recovery reads, and the plugin only takes that path (CrdtReplicaManager rebootstrapCanonicalAndForwardCapturedLocalEdit) when its replica is STALE against the canonical text. Healthy delivery reached the editor before edit two, so the recovery never had to run. DECISION NEEDED: (a) accept the healthy-path trail as the proof, extending verify-captured-splice-recovery with a capture/advance/capture triple, or (b) require the stale-replica race, making the recipe type edit two while the advance is still in flight. Aside: the scratch doc is under gitignored tmp/, so every advance ends commit_refused_ignored_path (write lands on disk, commit refused). Plan: tasks/agent-doc/plan-captured-editor-splice-recovery.md.
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically by a human reply on pypi/support#12214, or 'make pypi-quota-check' reporting a ceiling in reach. Rechecked 2026-09-30 16:25 EDT: 8.362 GiB of 10.0 GiB, 1.638 GiB headroom, 158 releases (0.35.113..0.35.424), post-cutover mean 34.7 KiB, ~49,485 further releases fit; VERDICT headroom fine. Nothing to act on or delete. pypi/support#12214 still open, 1 comment (github-actions bot), untouched since 2026-09-10T22:46:21Z. If the grant lands, record it and drop this item. Deletion stays reserved for a ceiling the check reports (next tranche 0.35.113..0.35.161, 2.491 GiB); verify any deletion from the authenticated /manage/project/agent-doc/ pages, never from pypi.org/pypi/agent-doc/json.
- [/] [#grokmcpverify] [operator-verify] Verify an authenticated Grok Build MCP admit/read/plan/finalize cycle and same-turn auto-queue continuation. NEW BLOCKER 2026-09-30: grok 1.0.41 is now SIGNED OUT ('grok -p' -> 'Error: Not signed in'); operator runs 'grok login --device-code' (or sets XAI_API_KEY), and the free usage quota or the planned subscription must be active. Grok CLI authorization, native JSON/backend commit fixtures, pane detection and quit behavior passed earlier; the last live MCP probe stopped at the usage limit before committed closeout. Heavy is not required. Plan: tasks/agent-doc/plan-grok-build-support.md.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#ghrecvpolicy] [agent-doc] Recipient-side [receive] auto_create_components policy for multi-user workspaces (GH #40, https://github.com/btakita/agent-doc/issues/40). Parked until multi-user workspace support exists.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
