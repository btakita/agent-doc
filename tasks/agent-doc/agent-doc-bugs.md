---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: c47dae65-e36f-40cb-8ebb-b7c9573e3c41
  codex: 01a10ac4-43d9-7292-a0c9-54c6148b42bb
agent: codex
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
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261004-070500.md`*

Compacted content:
- Archived 15 response topic(s): release + publish; release cleanup; queue dispatch; 12 more
- Prior summary/context: prior compacted content: Archived 33 response topic(s): GH #106, GH #107, and release v0.35.441; GH #108: JetBrains u...

### Re: do #steerbaselineabsorb — opus-5-5 · 2026-10-04T03:10-04:00

> **Queue prompt:** do [#steerbaselineabsorb]

I claimed this item and handed it to a background subagent (worktree `agent-doc-wt/steerbaselineabsorb`, branched from v0.35.450 `478ff964e`). It is fixing both symptoms:
1. **Steering absorbed into a commit:** steering that is still pending at closeout, but whose text is already in the committed baseline, will be saved as an unrecorded prompt (the same mechanism as `chat_prompts`). The next contract then carries it with `no_changes: false`.
2. **Answered steering keeps coming back:** an item is marked consumed once a later committed response answers it, so the hook stops resurfacing it.

It has two regression tests to add, one for each symptom, and must pass `make check` before committing on branch `steerbaselineabsorb`. When it reports back, I'll merge the branch, do one `make install`, release the claim, and close the item.

### Re: preflight refused on its own repair (haiven-dev sdk.md) — opus-5-5 · 2026-10-04T03:20-04:00

> **Chat prompt (#chatprompt):** Also fix in subagent:
> ❯ /agent-doc /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk.md
> ● Bash(agent-doc session-check tasks/sdk.md 2>&1 | grep -v '^\[perf\]' | tail -25)
>   ⎿ [session-check] ok — cycle `cycle-1791097819297` is `committed` (commit_success); repaired committed historical committed_capture snapshot drift
>      queue_continuation_required=true next_queue_prompt="Add backlog items for the demo work. Create an ADR PR in …
> ● agent-doc refused to start this turn, so I haven't written anything to tasks/sdk.md. Its own repair step left the file slightly different from what it expected.
>   I ran session-check afterwards. It fixed the mismatch and confirms my last reply (the demo table) is committed.
>   It also found your new queue item: "Add backlog items for the demo work. Create an ADR PR in the haiven-sdk repo." The loop restarts in about 90 seconds and will take that one first.

I filed this as `#preflightrepairdrift` and handed it to a second subagent in its own worktree (`agent-doc-wt/preflightrepairdrift`, branched from v0.35.450). It works separately from the `#steerbaselineabsorb` subagent.

The bug: preflight ran a repair step (fixing committed-capture snapshot drift), and then its own check refused the turn because the document no longer matched what it expected. session-check later made the same repair and reported ok, so the drift was harmless and the turn should have been admitted.

The subagent will:
1. Find the exact refusal reason in haiven-dev's `ops.log` (around 07:14Z).
2. Have preflight absorb its own repair before the admission check. Drift that would lose operator text still fails closed.
3. Add a regression test for this case and pass `make check`.

When both branches are in, I'll merge them serially and run one `make install`.

### Re: queue: GH #127, #128, #129 + release — opus-5-5 · 2026-10-04T03:52-04:00

> **Queue prompt:** do https://github.com/btakita/agent-doc/issues/127
> **Queue prompt:** do https://github.com/btakita/agent-doc/issues/128
> **Queue prompt:** do https://github.com/btakita/agent-doc/issues/129
> **Queue prompt:** release + publish

I claimed each head. Every subagent has its own worktree based on integration commit `b63deb809`, so it builds on both fixes that are not yet released:
- **GH #127** (`agent-doc-wt/gh127b`): `cross_document_sweep` uses 23–42s of the 85s admission deadline committing other documents. The fix takes it off the admission-critical path.
- **GH #128** (`agent-doc-wt/gh128b`): a stale-binary recycle reports `old_version == new_version`, then asks to restart again. The fix makes it report the version it actually re-exec'd into and stops the duplicate request.
- **GH #129** (`agent-doc-wt/gh129b`): the `session-check` repair hint asks for the very document write that the admission-deadline refusal forbids. The fix makes the two consistent and checks why the queue head was dropped.

**Both fixes are done and merged** on the integration branch (`b63deb809`), and the full `make check` is running on the merged code:
- **`#steerbaselineabsorb`** (`bc6abda76`): steering absorbed into a closeout commit now carries into the next contract with `no_changes: false`, and answered items stop resurfacing. Passed on its branch: 10822 tests.
- **`#preflightrepairdrift`** (`1c4caeded`): the haiven-dev refusal was the template-normalization repair. It ran while the operator was typing and correctly rolled forward over the new text, but the settle step only accepted the pre-typing target. Settle now accepts "the repair plus newer edits" and keeps the typed text as pending input. Passed on its branch: 10826 tests.

**release + publish** stays with me. Once the three GH branches land, I'll merge them in one at a time, run `make check` and one `make install`, cut the release, close the issues, and close all five items.

### Re: v0.35.451 released — #steerbaselineabsorb, #preflightrepairdrift, GH #127/#128/#129 — opus-5-5 · 2026-10-04T05:05-04:00

> **Queue prompt:** do [#steerbaselineabsorb]
> **Queue prompt:** do [#preflightrepairdrift]

All five fixes are integrated, released as **v0.35.451**, and installed (`agent-doc --version` → `0.35.451`).

**Integration.** I merged the five subagent branches serially onto one integration branch and added one follow-up commit (below). Full `make check` on the combined tree exited 0: **10859 tests run, 10859 passed**, 262 skipped. Release commit `db150df8c` and tag `v0.35.451` are pushed; the CI Release and PyPI workflows are running. I closed GH #127, #128 and #129 with a pointer to the release, and committed the superproject gitlink as `5575f53cf9`.

**What shipped:**
- **`#steerbaselineabsorb`:** when steering is still pending at closeout and its text is already in the committed baseline, it is saved in a per-document ledger. The next contract carries it as `absorbed_steering_prompts` / a `prompt_target` with `no_changes: false`. Once it is answered, no steering surface shows it again.
- **`#preflightrepairdrift`:** the haiven-dev refusal came from the template-normalization repair, which landed on top of text the operator typed after the repair was computed. Settle now accepts a result proven to be "the repair plus concurrent edits", and still fails closed when the repair is missing or the texts disagree. The typed text stays a pending diff.
- **GH #127:** `cross_document_sweep` now runs on its own budget: 8s by default and at most a quarter of the admission time left (`AGENT_DOC_PREFLIGHT_SWEEP_BUDGET_SECS`). Documents it doesn't reach are deferred to the next preflight through a cursor file. The hook raises its installed timeout to fit the budget.
- **GH #128:** the recycle itself was correct; only the report was wrong. The completion row now comes from the replacement's own status, and same-image recycles log as failures. A repeat restart for the same pid and generation within 30s is suppressed.
- **GH #129:** two new commands, `agent-doc backlog <FILE> requeue` and `keep-unqueued`, repair a dropped head without an admitted cycle. The deadline refusal now forbids only a response write. A queue head that leads with an open backlog id is no longer treated as free text, which was the likely way the head got dropped.
  - My follow-up commit fixes a second hint the subagent spotted: the no-response active-head hint now names `agent-doc backlog <FILE> done <id>`, which is allowed after a refused admission.

**Notes:**
- **Another session's uncommitted work in `src/agent-doc`:** a `lib.rs` edit (`#clearrestartjoinstall`) is still uncommitted there; I preserved it unchanged when fast-forwarding main. Its `VERSIONS.md` edit conflicted with the release entry. That edit looked garbled (it deleted the 0.35.449/450 entries), so I kept the release version. The original is in `stash@{0}` in the agent-doc checkout.
- **Queue items struck before the work was done:** my dispatch response quoted the free-text heads (GH #127/#128/#129, release + publish) as `> **Queue prompt:**`. That caused `#ftstrike` to strike them while they were only dispatched. The work is now done, so nothing was lost, but quoting a head that is only dispatched should not strike it.
- **Verify on the next session:** GH #128's 30s suppression means a newer client briefly connects to the older controller instead of retrying the handoff.

### Re: queue: About Agent Doc action, Dashboard — opus-5-5 · 2026-10-04T13:45-04:00

**`#editoractionmenu`: About Agent Doc.** I claimed it and handed it to a subagent working in its own worktree (`agent-doc-wt/editoractionmenu`, based on v0.35.451). The task: add an "About Agent Doc" action to each plugin's Agent Doc menu (JetBrains, VS Code, and Zed if it has a menu surface). It shows:
- the plugin version;
- the agent-doc binary version actually in use;
- the native library (cdylib) version and build id;
- a warning when the plugin and binary versions don't match.

It can add a CLI command such as `agent-doc version --json` if that helps. It will update the parity tables, bump the plugin versions, and run `make check` plus the plugin tests. I'll integrate the branch, install, and release.

**`#gvqv`: a half-typed fragment.** This was the start of your Dashboard request: preflight made backlog item `#gvqv` from `Add a ``` before you finished typing. I'm not dispatching it. The full line is still being written below it ("Add a `Dashboard` editor prompt + action to show the Agent Doc dashboard. Can we also support a dashboard md file that ?"). I'll dispatch the Dashboard request once that line settles, and close `#gvqv` into it.

I filed the fragment as a bug, `#halftypedcoin`: turning a free-text queue line into a backlog item should wait while the line is still being typed, or while it ends inside an unclosed code span.

**Install note.** The `stale_install` warning came up because I built v0.35.451 from a clean worktree, so the builds in `src/agent-doc`'s own target directory are older than its source. The binary actually in use is v0.35.451: `.bin/agent-doc` runs the newer `~/.cargo/bin/agent-doc`. The warning will clear when `src/agent-doc` is built again.

### Re: dispatched: Dashboard (#gvqv), #halftypedcoin; release queued — opus-5-5 · 2026-10-04T13:55-04:00

Three subagents are running, each in its own worktree based on v0.35.451:
- **`#editoractionmenu` (About Agent Doc):** `agent-doc-wt/editoractionmenu`.
- **`#gvqv` (Dashboard):** `agent-doc-wt/dashboardaction`. I saw you rewrite the `#gvqv` stub into the full request, so the free-text Dashboard head and `#gvqv` now go to this one subagent. It will:
  - add a "Dashboard" action, menu item and command-palette entry to the editor plugins;
  - combine `agent-doc board` (where the work is) with controller and supervisor liveness;
  - generate a live-updating markdown file under `.agent-doc/`, written atomically and kept fresh by the binary, that is never scanned as a session document and that the action opens;
  - add a matching `agent-doc dashboard` command.
- **`#halftypedcoin`:** `agent-doc-wt/halftypedcoin`. Turning a free-text queue line into a backlog item will wait until the operator stops typing, will never act on an obviously unfinished line like `Add a ```, and will fold later typing into the item it created.

**release + publish** stays with me. Once all three branches report back, I'll merge them one at a time (the two plugin branches will need their version bumps and parity tables reconciled), run `make check` and one `make install`, release v0.35.452, and close the items.

### Re: re-trigger while v0.35.452 integrates — opus-5-5 · 2026-10-04T14:40-04:00

Nothing new needs answering this cycle. Every live queue head is in flight:
- `#editoractionmenu`, `#gvqv` and `#halftypedcoin` are merged into the integration branch `integ452`.
- `release + publish` is mine.
- Full `make check` is running on the combined tree.

**The "prompt" this cycle carried is your `#gvqv` backlog edit.** Preflight listed `- [ ] [#gvqv] Add a `Dashboard` editor prompt ...` under `absorbed_steering_prompts`. That line is a backlog item, not an exchange prompt, and the Dashboard work is already merged. Two bugs combine here:
1. The steering report has been labelling that backlog edit `source=exchange` since you made it.
2. The new absorbed-steering ledger from v0.35.451 trusted that label.

I filed this as `#steerbacklogsource`. Steering should be attributed to the component that actually changed, and the ledger should record only exchange lines.

`do [#gvqv]` also appears twice in the queue: your rewritten stub plus the mirror. Both heads close together when `#gvqv` is marked done at release.

**Next:** when the suite passes, I'll fast-forward main, run one `make install`, and release v0.35.452 with the missing 0.35.451 parity row. Then I'll close `#editoractionmenu`, `#gvqv` and `#halftypedcoin`, and dispatch `#steerbacklogsource`.

### Re: v0.35.452 released — About Agent Doc, Dashboard, #halftypedcoin — opus-5-5 · 2026-10-04T15:20-04:00

> **Queue prompt:** do [#editoractionmenu]
> **Queue prompt:** do [#gvqv]
> **Queue prompt:** do [#halftypedcoin]
> **Queue prompt:** release + publish

**v0.35.452 is released and installed.** `agent-doc version --json` reports `0.35.452`, with JetBrains plugin `0.2.488` and VS Code extension `0.2.80`. The JetBrains plugin was swapped into the running IDE without a restart, and the VS Code extension is installed in codium.

**Verification.** I merged the three branches into `integ452`. The two plugin branches conflicted in 9 places (menus, popup lists, spec sections), and I resolved each one to keep both actions. Because the combined plugin code is new, I re-bumped JetBrains to 0.2.488 and VS Code to 0.2.80. Full `make check` exited 0 with **10890 tests run, 10890 passed**; that includes the JetBrains and VS Code suites. `editor-parity` and the plugin version fence pass on the release commit. Release commit `d08511f2b` and tag `v0.35.452` are pushed, and the superproject gitlink is `57cef3200f`.

**What shipped:**
- **About Agent Doc** (`#editoractionmenu`):
  - New `agent-doc version --json` and a matching FFI, both reporting the version, build id, binary path, native library, and expected plugin versions.
  - JetBrains and VS Code get an About action that shows the plugin, CLI, and native-library versions and warns on mismatch. Zed has no action surface and reports its version through LSP `serverInfo`.
- **Dashboard** (`#gvqv`):
  - `agent-doc dashboard` shows the fleet work board plus controller and supervisor liveness.
  - Once `.agent-doc/dashboard.md` exists, the project controller keeps it live. It is excluded from session scanning by a marker line.
  - The Dashboard action writes the file and opens it. Note that `dashboard` is no longer an alias for `board`.
- **`#halftypedcoin`:** a line ending in an empty `` `` `` pair now counts as unfinished. Coining a free-text line respects the typing gate, and a stub absorbs the line you kept typing.
- **Release-parity row:** I added the 0.35.451 row that my v0.35.451 release omitted. That gap had been failing `make check` on every branch.

**Still in flight:**
- **GH #130:** the controller closeout gate can't release an owner whose process has died. A subagent is working on it in `agent-doc-wt/gh130`.
- **`#steerbacklogsource`:** a subagent is running its `make check` in `agent-doc-wt/steerbacklogsource`.

Both will ship together in the next release.

### Re: queue: #ghbrew, #ghjbfolding, #ghmultitmux, #apimdrevert + release — opus-5-5 · 2026-10-04T15:25-04:00

> **Queue prompt:** do [#ghbrew]
> **Queue prompt:** do [#ghjbfolding]
> **Queue prompt:** do [#ghmultitmux]
> **Queue prompt:** do [#apimdrevert]

I claimed and dispatched each item to its own subagent, each in a worktree based on v0.35.452 (`agent-doc-wt/<id>`):
- **`#ghbrew` (GH #31):** `Formula/agent-doc.rb` in the `homebrew-tap` repo (branch `agent-doc-formula`), built from the release archives: binary and cdylib side by side, with sha256s. It also gets a release-workflow step that bumps the formula on each tag. Pushing the tap and closing #31 stay with me.
- **`#ghjbfolding` (GH #19, phases 5–7):** folding for `agent:*` regions, gutter icons on component markers, and a structure view of components, with JetBrains tests.
- **`#ghmultitmux` (GH #17):** an allowed-sessions list in config, with routing, start, sync and layout choosing among the sessions. It fails closed on a session not in the list, the single-session default is unchanged, and SimWorld tests cover it on isolated tmux sockets only.
- **`#apimdrevert`:** read-only forensics first. The subagent checks JetBrains Local History and `idea.log` around 2026-09-29 00:40, the git history of `api.md`, and haiven-dev's `ops.log`, to answer the open question itself: was operator text actually lost? If it finds a real path that drops text, it fixes it with a regression test. If not, the item closes as not-a-defect, with the evidence.

> **Queue prompt:** release + publish

**Deferred, not done:** this release has not happened yet. It is claimed by me (`coordinator:integration`) and waits for the six in-flight branches: the four above plus GH #130 and `#steerbacklogsource`. When they land, I'll merge them one at a time, run `make check` and one `make install`, release v0.35.453, and close the items and GH issues. Keep this head open until then.
### Re: try again — opus-5-5 · 2026-10-04T13:57-04:00

> **Chat prompt (#chatprompt):** try again

Retrying. The document was stuck in an interrupted state: `session-check` reported that the editor authority and the disk copy disagree in one place. The queue on disk has `- do [#deferstrike]` (written there when I filed `#deferstrike`), but the editor buffer doesn't. The log also showed `live_editors=0` at that moment. `session-check`'s prescribed remedy is to open a new cycle, and this reply is that cycle: it commits whatever is currently in the buffer instead of forcing the disk copy.

Integration status: all six branches (`#ghjbfolding`, `#ghbrew`, GH #130, `#steerbacklogsource`, `#ghmultitmux`, `#deferstrike`) are merged on `integ453`. The serial `make check` on it is still running (10,921 tests); nothing else is building now, and the disk has 1.1T free. When it passes, I'll release v0.35.453 and close the items.

### Re: try again — v0.35.453 closed out — opus-5-5 · 2026-10-04T14:15-04:00

> **Chat prompt (#chatprompt):** try again

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/130

Done. After your IDE restart the editor re-registered: 6 live registrations, and the earlier "try again" reply committed. This cycle closes out **v0.35.453**, which is already released, installed and pushed.

**Released in v0.35.453** (release commit `8df52fee7`, tag pushed, superproject gitlink `84f3b3aca0`). All six branches were merged serially; full `make check` exited 0 with **10921 tests passed**, and `editor-parity` and the plugin version fence passed. Installed: CLI `0.35.453`, JetBrains `0.2.490`, VS Code `0.2.80`.
- **GH #130, closed:** the controller closeout gate now checks whether the owner process is still alive, so a claimant blocked on an owner that has exited is released right away (`OwnerProcessGone`) instead of after the 300s lease.
- **`#ghmultitmux` (GH #17, closed):** a `tmux_sessions` allow-list, plus a per-document `tmux_session:` binding that one policy applies to route, sync, start, claim, `tmux ensure`, `session set` and terminal. It fails closed on a session not in the list, and single-session projects are unchanged. Runbook: `runbooks/multi-tmux-sessions.md`.
- **`#ghjbfolding` (GH #19):** JetBrains component folding (`agent:NAME · N items`), gutter toggles and a structure view, built from the native parser off the editor thread. The plugin needs a live-IDE look.
- **`#ghbrew` (GH #31, closed):** the formula is pushed to `btakita/homebrew-tap` main, so `brew install btakita/tap/agent-doc` works on Linux. `homebrew.yml` bumps it on each tag once the `HOMEBREW_TAP_TOKEN` secret is added; macOS waits on Darwin archives.
- **`#steerbacklogsource`:** edits in backlog/review/done/queue are no longer surfaced as exchange steering or carried forward as absorbed prompts.
- **`#deferstrike`:** claimed or `**Deferred:**` free-text queue heads are no longer struck, and the pre-write gate skips claimed heads.
- **`#apimdrevert`, closed as a real loss that was already fixed:** two pieces of operator text in `api.md` were lost on 2026-09-28 at 21:34 and 21:52, when the editor reattached after an install. They were the words " Add alerts " and the queue's `go` marker plus `do [#sbxsignupcodegate]` and `do [#sbxearlyaccesssheet]`. The cause was fixed that same night (`f812e450d`, then `c9b731ea1`). The lost text was not restored; re-add it by hand if you still want it.

**New bugs filed from this session** (appended to the backlog):
- **`#jbpluginvanish`:** the JetBrains plugin directory disappeared around the 13:49 IDE restart, while the disk was full, and every document lost its editor. I reinstalled it from the release.
- **`#jbdynamicfalsereport`:** `plugin install` claimed it had "dynamically replaced, no restart required" when no plugin was loaded in the IDE.

### Re: JetBrains plugin vanish + false dynamic-replace report — opus-5.5 · 2026-10-04T15:03-04:00

> **Queue prompt:**
>
> do [#jbpluginvanish]
>
> do [#jbdynamicfalsereport]


> **Chat prompt (#chatprompt):** Watch agent-doc github issues for new issues.

> **Chat prompt (#chatprompt):** I will have another process file an issue in the coder environment. This will take ~20-30 minutes

**Issue watch:** a background poller checks `btakita/agent-doc` every 5 minutes for any issue newer than #133 and wakes this session when one appears; each new issue is dispatched to its own subagent. GH #133 (route-owned supervisors never reaped / respawn on kill) had just been filed and is already with a subagent.

**`#jbpluginvanish` — done** (agent-doc `1b1088b36`, merged to main):
- **Root cause:** not the full disk. A *stale 0.2.468 restart staging* was still queued in the IDE `action.script` (`delete:<plugins>/agent-doc-jetbrains` then `unzip:<0.2.468 zip>`). The 13:12 restart-free upgrade never cleared it. At the 13:48 restart the IDE ran the delete, the zip was already gone, the unzip failed, and the plugin was gone (`idea.1.log`: `Source file missing: …agent-doc-jetbrains-0.2.468.zip`). If the zip had survived, the restart would have *downgraded* to 0.2.468 instead.
- **Fix 1:** every install purges agent-doc stagings whose zip is gone. After a successful restart-free upgrade or a direct replace, it purges all agent-doc stagings for that plugins dir. The `action.script` rewrite is atomic and leaves other plugins' lines unchanged; each purge logs `plugin_pending_staging_purged`.
- **Fix 2 (latent bug):** `replace_jetbrains_plugin_tree` used to `remove_dir_all` before extracting. It now extracts into a staging dir, fsyncs and checks the tree against the package, moves the old tree to a backup, swaps the new one in, and restores the backup on failure. A backup left by an interrupted swap is restored on the next attempt.
- **Tests:** 6 swap tests (disk full mid-extraction, failure moving the old tree aside, failed swap, package with no plugin jar, successful swap, interrupted-swap recovery) + 3 purge tests (including a replay of the exact 0.2.468 incident script). Putting the old delete-first code back failed 5 of the 6 swap tests.
- **Follow-up `#jbstagebackup`** (added): the Java restart staging still has to emit delete-then-unzip. Preflight should auto-purge doomed stagings instead of only warning, and/or the staging should keep a backup copy.

**`#jbdynamicfalsereport` — done** (agent-doc `d28a000d7`, merged to main):
- **Root cause:** the upgrader's `skip:plugin-not-loaded` answer was discarded (only `ok:`/`staged:` were parsed). The install then fell through to a plain `Installed`, and `jetbrains_convergence_restart_summary` printed "dynamically replaced; no IDE restart is required" for any changed install.
- **Fix:** upgrader answers are parsed into typed outcomes. An `ok:` counts as a dynamic replacement only once the IDE process is seen with the new `agent-doc-jetbrains-<version>.jar` mapped in memory (checked every 200ms for up to 10s). Otherwise the install reports restart required. "Plugin not loaded" also reports restart required. The Java side now includes its plugins dir in the skip answer, so an IDE that uses a different plugins dir is ignored. Plugin bumped to 0.2.491.
- **Tests:** not loaded → restart required; `ok:` with no live-load proof → restart required, with no second rewrite; proof seen → dynamic; status parsing; mapped-jar matching.

**Verification:** `make check` on merged main: nextest **10936 passed, 0 failed**. The final `audit-docs` step failed only on the stale installed `.grok` skill and passed after `make install`. `make install` EXIT=0. The live install was itself a proof of the new path: `Plugin dynamically upgraded in 1 live JetBrains process(es)` → `dynamically replaced and verified live`. `IntelliJIdea2026.1/agent-doc-jetbrains/lib/agent-doc-jetbrains-0.2.491.jar` is present and no `action.script` is pending.

**Still in flight (claimed, subagents):** GH #131 (rejected editor IPC receipt), GH #132 (upgrade cdylib + gc-libs), GH #133 (supervisor reap/respawn). They will be integrated as one batch, followed by one `make install`, then `release + publish`.

### Re: GH #131–#134 batch — opus-5.5 · 2026-10-04T15:40-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/131

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/132

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/133

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/134

All four were fixed by background subagents in separate worktrees, then merged serially into agent-doc main. **Full-batch `make check`: nextest 10972 passed, 0 failed, EXIT=0 (audit-docs + TLA green). `make install` EXIT=0:** `reload_library` delivered 1/1, and JetBrains plugin 0.2.492 was `dynamically replaced and verified live`.

- **GH #131 — rejected editor receipt** (`2bedfb560`): a typed `SocketDeliveryFailure::{Timeout,Rejected,Unsupported}`; rejections count toward degraded/recycle at all three recording sites, and a new `consecutive_rejections` column records them. The single ownership predicate (`agent_doc_turn::write_ownership`) gains a `DeliveryRejected` verdict: a write waiting on an endpoint that answered NO is no longer "deferred". Its guidance now names the recovery (restart/reload that editor, then `session-check`) instead of forbidding every one. 5 regression tests, including the real refusal against a state.db row. Follow-up **`#gh131nonipc`**: retention paths that never reach IPC still record no transport health.
- **GH #132 — cdylib pile-up** (`165cdec4a`): `agent-doc upgrade` no longer untars the library over `~/.cargo/bin`. It stages, installs through `lib_install::install_versioned` (a versioned file plus an atomic symlink swap that migrates a plain file in one rename), installs the library before the binary, then runs `gc-libs` automatically. `gc-libs` now recognises a plain-file install (hard-link or byte-identical copy) and sweeps markers left by dead pids. 7 tests.
- **GH #133 — supervisor reap/respawn** (`cc24eb23f`):
  - Every route-owned supervisor now checks its owner record every 15s. It reaps itself (`superseded_binding_orphan`) only when the record names a *different*, live pane, no cycle is open, and its pane is idle.
  - The controller watchdog respawns a supervisor only when its pane is outside a stash window (`#watchdogbindinglive`).
  - Tests include a mutation proof.
  - Follow-up **`#gh133sigterm`**: a deliberate kill in a visible column still respawns, and there is an open policy question about stashed owned panes.
- **GH #134 — Remote Dev layout `unknown`** (`b07ac304c`):
  - The JetBrains plugin now reports each remote client's visible, selected and open editors.
  - A new pure resolver, `agent-doc-editor-surface/src/remote_layout.rs`, reads that as one column per visible document. It keeps a known split while the evidence is thin, and a tab switch replaces a column rather than adding one (no-growth, GH #120). A cold start with no evidence stays `unknown` (GH #105).
  - The resolver is reached over FFI, with a fallback if the native library is older. Plugin 0.2.492. 11 resolver tests.
  - **[operator-verify]:** in a live Remote Dev session with two docs in two splits, look for `source=remote_client_visible_editors` → `layout=observed`.

**Integration note:** another session has uncommitted WIP in agent-doc `agent-doc-start-runtime-io/src/lib.rs` (#clearrestartjoinstall). It overlapped GH #133's file, so I merged on an `integ454` worktree and fast-forwarded main. The WIP patch was backed up, verified to apply cleanly on top, and re-applied unchanged.

**Issue watch:** still polling for issues above #134. Next: `release + publish` (v0.35.454).

### Re: release + publish v0.35.454 — opus-5.5 · 2026-10-04T16:05-04:00

> **Queue prompt:** release + publish

**Released v0.35.454** (agent-doc `ccdb77886`, tag `v0.35.454` pushed; superproject submodule bump `3d7959bfa9` pushed):
- `make release-version VERSION=0.35.454`, plus a `VERSIONS.md` § 0.35.454 entry covering jbpluginvanish, jbdynamicfalsereport and GH #131–#134.
- The first `make check` failed at the editor parity audit: a release needs a row in `editors/release-parity.tsv`. I added the 0.35.454 row.
- Re-run: **nextest 10972 passed, 0 failed, CHECK_EXIT=0**, then `make install-full` INSTALLFULL_EXIT=0 (`agent-doc 0.35.454` installed, `reload_library` 1/1, JetBrains already current at 0.2.492).
- I ran the tag + push directly instead of `make release`, which would have re-run `check` and `install-full` on an identical tree.
- The release commit leaves out the other session's uncommitted `agent-doc-start-runtime-io/src/lib.rs` WIP and `.claude/settings.json`; both are still in its working tree.

**CI (observed once, not awaited):** the Release (37230736724) and CI (37230736490) runs are in progress; Book is green.

**GitHub:** #131, #132 and #133 are closed with "Fixed in v0.35.454". #134 was already closed; I added a fix comment with the live Remote Dev verification steps.

**Defect found → `#unstrikelost` (added):** your 14:13 unstrike of `release + publish` was undone by the 14:31 session-doc commit, although no response had answered that head. It only ran because I noticed the queue was empty with no release done.

**Issue watch:** still polling for issues above #134.

### Re: queue fan-out — #unstrikelost, #gh131nonipc, #gh133sigterm, #jbstagebackup, release — opus-5-5 · 2026-10-04T21:48-04:00

Queue preset `#subagents #spec-test-build-install-commit-push`. I'm coordinating: each item is claimed and sent to its own background subagent. Each subagent has its own git worktree in `~/worktrees/agent-doc-<id>`, branched from `origin/main`, and pushes its own branch. Subagents do not run `make install` or release anything.

| Head | Owner | Branch |
|---|---|---|
| `#unstrikelost` | subagent:unstrikelost | `unstrikelost` |
| `#gh131nonipc` | subagent:gh131nonipc | `gh131nonipc` |
| `#gh133sigterm` | subagent:gh133sigterm | `gh133sigterm` |
| `#jbstagebackup` | subagent:jbstagebackup | `jbstagebackup` |

> **Queue prompt:** release + publish

**Deferred:** I claimed this one (`subagent:release-coordinator`) but am not running it in a subagent, because subagents never cut releases. When all four branches report back, I'll merge them into `main` one at a time, run `make check`, run one `make install`, then do the version bump, tag and publish myself.

Notes:
- The main `src/agent-doc` checkout has an uncommitted `agent-doc-start-runtime-io/src/lib.rs` from another session. The subagents work from `origin/main` and don't touch it.
- There's a stray `- release + publish` line under `## Completed / Reaped`, after the `agent:done` block. It sits outside every managed component, so the queue ignores it. Delete it if it was a mis-paste.
- Backlog line 333 (`6local-splice-batch 3 splices …`) is a truncated fragment from earlier. It has no `[#id]` checkbox, so it was there before this cycle.

### Re: GH #135 dispatched — opus-5-5 · 2026-10-04T22:01-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/135

**Deferred:** I claimed this as `subagent:gh135` and sent it to a background subagent. It works in the worktree `~/worktrees/agent-doc-gh135`, on branch `gh135`. The fix is an age-based sweep that deletes `turn_active` leases older than `TURN_ACTIVE_TTL_SECS`, run from the preflight auto-GC boundary, so a lease whose pane has died is still removed. It comes with spec and tests. Under `#gh-fix` ("fix then close+release"), I'll close the issue after the batch release, not before.

The four earlier subagents (`#unstrikelost`, `#gh131nonipc`, `#gh133sigterm`, `#jbstagebackup`) are still running and are waiting on their `make check` runs. **release + publish** stays with me until all five branches are merged. The exchange section is large (298 lines, 15 responses), but I'm not compacting while the queue is active.

### Re: release status — opus-5-5 · 2026-10-04T22:46-04:00

> **Chat prompt (#chatprompt):** did you release + publish?

Not yet. Nothing is tagged or published, and `main` is still at v0.35.454. The release head is still claimed by me (`subagent:release-coordinator`). I'm holding it until the integrated batch passes a full suite run.

All five branches, plus the `#unstrikelost` follow-up, are merged into the integration branch `integ455` (worktree `agent-doc-wt/integ455`). I'm running `make check` on it now, without retries: 11,010 tests across 180 binaries. Once it passes, I will:

1. fast-forward `main` and push;
2. run `make install` once and recycle;
3. bump the version and add the `VERSIONS.md` entry, then tag v0.35.455 (pushing the tag publishes to GitHub Releases and PyPI);
4. close GH #135, which is what `#gh-fix` asks for, and mark `#unstrikelost`, `#gh131nonipc`, `#gh133sigterm` and `#jbstagebackup` done.

### Re: batch integrated, v0.35.455 released — opus-5-5 · 2026-10-04T22:58-04:00

All five subagent branches, plus a follow-up fix for `#unstrikelost`, are merged into `main` and shipped as **v0.35.455**. The full `make check` on the integrated branch passed cleanly, with no retries: **11,010 tests run, 11,010 passed**, exit 0. Every subagent's own run had hit timing flakes at load average 60–100, so this clean run is the evidence for the release.

> **Queue prompt:** release + publish

Done:
- **Release:** commit `00d0468df` (`chore: release v0.35.455`) is pushed to `main`, and tag `v0.35.455` is pushed. The GitHub Release and PyPI runs started from the tag and were still in progress when this response was committed.
- **Install:** `make install` was run from the clean release tree, not the main checkout, so the other session's uncommitted `agent-doc-start-runtime-io/src/lib.rs` edit was not built in. `agent-doc --version` reports 0.35.455. The library reload reached the 1 open editor. The VS Code extension is now 0.2.80.
- **JetBrains plugin:** the install's source-change check bumped the plugin to **0.2.494**, so I committed that bump as `ffe836933`. The tag still says 0.2.493.
- **Superproject:** the submodule bump is committed as `da34bb1d6` and pushed.
- **WIP preserved:** the main `src/agent-doc` checkout is fast-forwarded. The other session's uncommitted edit was stashed and restored on top unchanged; a backup copy is in my scratchpad.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/135

**Fixed and closed.** Merged commit `a0f59fd93`. Expired `turn_active` leases are now swept by age, in preflight's automatic cleanup and in `agent-doc gc`. Any read that finds an expired lease also deletes it. Removal no longer depends on the pane that created the lease, so a lease whose pane died is cleaned up too. GH #135 is closed with a comment pointing at the fix.

**`#unstrikelost`** (`e70ac0d19` + `e76fcf260`): the operator's unstrike was undone by preflight's queue clean-up, not by the closeout. That step re-strikes any head that is struck in the last commit but live in the editor, to keep a stale buffer from reviving finished work. An operator re-arm looks identical, so it was struck again.
- Preflight now replays the operator's recorded keystrokes onto the last commit and leaves alone any head they re-armed.
- The follow-up applies the same keystroke check to the closeout merge, which had the same struck-beats-live rule.
- Regression tests reproduce both paths and fail when the keystroke check is turned off.

**`#gh131nonipc`** (`db988678b`): two failures that never reached the socket are now recorded in editor transport health: a visible write refused as divergent, and a `serialized_atomic_write` that is held back waiting for the editor.
- After 3 refusals in a row, the editor connection is treated as unregistered. A proven delivery resets the count, and after 60s one probe write is allowed through.
- An ACK no longer clears a refusal count that is still building.

**`#gh133sigterm`** (`7095bbdef`): a supervisor you SIGTERM on purpose now records an exit marker tied to its pid and generation.
- The controller watchdog honours the marker (`reason=intentional_exit`). `admin kill-supervisor` records the same marker.
- Recycle/replace clears its own marker before starting the replacement, and a crash still respawns.
- Stashed idle owned supervisors are still not reaped, because they hold the document's live conversation; this is now stated in the spec.
- The actual replacement cold start is compiled out of test builds, so it has not been exercised.

**`#jbstagebackup`** (`c53aabbb5`): the restart staging now unzips the package once into a test folder before deleting the installed plugin. A vanished or corrupt zip, or a full disk, now stops the script before anything is deleted.
- Preflight repairs broken stagings and test folders, and removes orphan agent-doc zips.
- The stale `agent-doc-jetbrains-0.2.469.zip` was deleted after confirming no pending `action.script` referenced it.
- One remaining gap is filed as `jbstagefallback` below.

Follow-ups and notes:
- **`jbstagefallback` (backlog):** when IntelliJ's command classes cannot be built, the plugin falls back to `installAfterRestart`, which still writes the old, unprotected order. It is added to the backlog and the queue.
- **Flaky test:** `finalize_editor_absent_replays_operator_deleted_struck_queue_rows` failed under load in two separate subagent runs. It passed in the clean integrated run. If it shows up again, it is worth checking for a timing dependency.
- **Size:** the exchange section is large (over 300 lines); I am not compacting while the queue is active.

### Re: GH #131 reopened, dispatched; release held — opus-5-5 · 2026-10-04T23:23-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/131

**Deferred:** I claimed this as `subagent:gh131reopen` and sent it to a background subagent. It works in `~/worktrees/agent-doc-gh131reopen` on branch `gh131reopen`, starting from v0.35.455. The reopened issue (comment 2026-10-05T03:05Z) shows the recovery guidance never terminates:

1. **The two repairs point at each other.** `session-check` names `write --done <id> --pending-only --commit`. That command refuses with the retained-write deferral, which sends you back to `session-check`.
2. **`admin reload-lib` is both required and forbidden.** The integrity gate prescribes it, and it was the step that actually worked. The retained-write guidance forbids it.
3. **Preflight refuses the whole turn.** It rejects turns because a retained write still owns a captured response, even though that response is already committed in HEAD.

v0.35.455 already made the transport degrade on refusals (the `#gh131nonipc` fix). This round has three goals:
- a pending-only mutation for an already-committed response is no longer blocked by that response's retained write;
- `session-check` and the write path give consistent recovery advice, including on `reload-lib`, derived from the shared ownership predicate;
- preflight admits the turn while that write finishes.

Each of the three shapes gets a regression test that fails without the fix. This turn's own earlier closeout hit shape 1 (`tracked-work mutations ... retained`, then a refused close).

> **Queue prompt:** release + publish

**Deferred:** I claimed this as `subagent:release-coordinator`. v0.35.455 is already out: the GitHub Release has 8 assets and PyPI has 0.35.455. This head stays queued for **v0.35.456**, which I'll cut after merging `gh131reopen`, running a clean `make check` and one `make install`. GH #131 gets closed at that release.

### Re: GH #131 fixed, v0.35.456 released — opus-5-5 · 2026-10-05T00:12-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/131

**Fixed and closed.** Fix commit `3443fe555` is merged into release `504ed3089`. GH #131 is closed with a comment. The three recovery loops now each finish, and all derive from the shared `write_ownership` predicate:
- **Pending-only writes are absorbed.** A pending-only `--done/--pending-gate … --pending-only` write is no longer refused when its response is already committed. It becomes a pending-only commit continuation, and `session-check` commits it once delivery converges. A pending-only `--done` stopped by this specific refusal used to have nothing that would ever commit it.
- **`session-check` stops naming refused repairs.** It no longer suggests a repair the write path will refuse (`tracked_work_repair_admission`). When the editor connection rejects writes or the editor isn't serving its replica, it prints the editor recovery first.
- **The `admin reload-lib` advice is consistent.** A new `ReplicaUnserved` verdict renders one shared recovery sentence for both the integrity gate and the retained-write message. That message no longer forbids reload-lib when `session-check` reports the editor isn't serving its replica.
- **Preflight admits the turn.** It no longer refuses because a pending-only write's base already contains a committed response. The write that introduced the response body still blocks, as before.

Each case has a regression test that fails with its fix reverted.

One known gap: "editor isn't serving its replica" is read from an attachment flag that can stay set after the editor closes. In that case the advice says reload-lib, which won't help. The integrity gate has the same weakness, so the two still give the same advice.

> **Queue prompt:** release + publish

**Done: v0.35.456.**
- **Release commit and tag:** `504ed3089` is pushed to `main`, and tag `v0.35.456` is pushed. The GitHub Release and PyPI runs were still in progress when this was committed.
- **Verification:** `make check` ran on the bumped release tree before the commit. 11,019 tests passed with no retries, the editor-parity check passed, and the run exited 0.
- **Install:** done from the clean release tree; `agent-doc --version` reports 0.35.456. The reload reached the 1 open editor, and the JetBrains plugin stayed at 0.2.494.
- **Superproject:** the submodule bump is pushed. The other session's uncommitted `agent-doc-start-runtime-io/src/lib.rs` edit is still unchanged in the main checkout.

**A mistake in my 0.35.455 release:** it was missing its row in `editors/release-parity.tsv`. My `make check` ran before the version bump, so the editor-parity check never covered it, and CI on `main` went red. I added the row as `13b9f9726`. For 0.35.456 I bumped the version and added both rows *before* running `make check`, so this run did cover them.

### Re: queue go — subagent dispatch for 3 heads, release held — opus-5-5 · 2026-10-05T00:22-04:00

Exchange is large (406 lines / 20 sections); still draining.

Three heads went to background subagents. Each one is claimed and has its own worktree under `~/work/btakita/agent-doc-wt/`, outside the IDE-watched project. Subagents only commit to their own branch. They don't push, install or release. I'll merge their branches one at a time and run one `make install` for the whole batch.

| Head | Claim owner | Worktree / branch |
|---|---|---|
| `do [#jbstagefallback]` | `subagent:jbstagefallback` | `agent-doc-wt/jbstagefallback` |
| `#gh-fix` GH #136 (stale stash supervisor pane admitted as a layout column) | `subagent:gh136` | `agent-doc-wt/gh136` |
| The alt+shift agent-doc menu is not coming up. | `subagent:altshiftmenu` | `agent-doc-wt/altshiftmenu` |

> **Queue prompt:** release + publish

**Deferred:** this release needs to ship the three fixes above. It runs after I've merged their branches, run `make check` and `make install`. GH #136 closes only after the release (`#gh-fix` → fix then closerelease).

### Re: release held until subagent batch integrates — opus-5-5 · 2026-10-05T00:17-04:00

> **Queue prompt:** release + publish

**Deferred:** this release has to ship the three subagent fixes (`#jbstagefallback`, GH #136, alt+shift menu), and all three are still running. I've claimed the head as `coordinator:after-subagent-integration` with a 3h TTL so the loop stops re-entering for it. Re-entering would only produce no-op cycles, and there have already been 2 in the last 30 minutes. Once the subagents report back, I'll merge their branches one at a time, run `make check` and one `make install`, then release this claim and cut the release.

### Re: Coder + Zscaler latency as the design point for GH #136 — opus-5-5 · 2026-10-05T00:27-04:00

> **Chat prompt (#chatprompt):** Note that coder has latency. The environment also has zscaler which adds more latency.

I've forwarded this to the GH #136 subagent along with the earlier formal-proof request. The design point is now the remote path (Coder workspace, JetBrains Remote Dev backend, Zscaler), not localhost:

- **No timing dependence:** state transitions are driven by events and acknowledgements. The invariants must hold under any message delay or reordering between the plugin, the controller and tmux.
- **Lower latency means fewer round trips, not shorter timeouts:** remove the serial round trips and fixed waits in the focus/layout effect path (the 60s focus settle, the `retry_pending` → `converged` second attempt). Timeouts that absorb network delay stay as they are.
- **Configurable timeouts:** any timeout or budget that gets added is configurable or based on measured round-trip time, and the spec names the remote/Zscaler path as the design point.

Batch status: `#jbstagefallback` and the alt+shift menu fix are merged on `batch-0505` (plugin 0.2.496). The JetBrains suite passes there (exit 0, 714 tests, 0 failures). GH #136 is still running, and the release stays held until it lands.

### Re: provable correctness under varying network conditions — opus-5-5 · 2026-10-05T00:28-04:00

> **Chat prompt (#chatprompt):** Basically agent-doc needs to be provably correct with varying network conditions.

There's a real gap here. `formal/tla/` has about 30 models, each with Reach/Wedge variants, but they treat the editor↔controller↔tmux IPC hop as either an atomic step or a perfect channel. The models README never mentions delay, reordering, loss, duplication or reconnect. Their safety proofs hold only for a perfect network, which is exactly what the Coder + Zscaler path isn't.

The target property set, from the plan file `tasks/agent-doc/plan-network-adversarial-correctness.md`:

- **Safety under an adversarial channel:** any delay, reordering, loss, duplication, or reconnect to a new generation. No timeout value may appear in a safety argument.
- **Liveness under fairness only:** a message re-sent infinitely often is eventually delivered. Progress may wait on delivery, never on a wall-clock guess.
- **Latency:** each hot path has a stated, tested serial round-trip count. Any remaining timeout is configurable or derived from measured RTT.

These are captured as phased backlog items, appended so they don't jump ahead of the current batch or the release:

- `netadv1`: audit each model's and protocol's channel assumptions.
- `netadv2`: a shared `NetChannel.tla` module, with Wedge configs proving it can catch a timing bug.
- `netadv3`: re-check the hot-path models over `NetChannel` and fix every counterexample, with SimWorld replay.
- `netadv4`: seeded network fault injection in SimWorld, with a Coder+Zscaler profile, run in `make check`.
- `netadv5`: tests that enforce the round-trip budget per path, and ack-driven transitions in place of fixed waits.

GH #136 is the first concrete case. I've told its subagent to model the messages as an adversarial channel: invariants hold under delay, reordering, drop, duplication and reconnect; liveness only under fairness. It should also add a Wedge config showing the pre-fix behaviour breaks the invariant. The plan file is committed (`992e5902c7`).

### Re: message loss over the network — opus-5-5 · 2026-10-05T00:29-04:00

> **Chat prompt (#chatprompt):** Messages may also be lost over the network?

Yes. The plan's adversarial channel includes a Drop action, and loss is the harder case. A delayed or reordered message still arrives eventually, but a lost one never arrives unless something resends it. Loss requires every hot-path protocol to have these properties (now explicit in the plan, commit `9ffa70872b`):

- **Resend until acked, or pull the state.** A change that crosses the network is either resent until acknowledged, or held as durable state the receiver reads and reconciles. A one-shot fire-and-forget notification on a correctness path is a defect.
- **Idempotent receivers.** The sender can't tell a lost request from a lost ack, so it resends, and the receiver applies each message exactly once by key (idempotency key, sequence number or generation), even when duplicated.
- **Half-open connections.** Zscaler can silently stall a stream with no reset. A heartbeat/lease turns that into a reconnect, and safety never depends on how fast the heartbeat fires.
- **Reconnect resync.** After a reconnect, the two sides reconcile from durable state (`state.db`, generation counters), never from messages assumed to be in flight.
- **Liveness only under fairness:** a message resent infinitely often is eventually delivered. A Wedge config with a fire-and-forget variant must show the stall, so a green run is not vacuous.

GH #136 may itself be a loss bug. Its safe-boundary recycle request was never consumed (`prior_request=unconsumed`, up to 22h old), which is what you'd see if a one-shot notification were dropped. I've told that subagent to check whether this is the cause. If it is, the fix should be a durable request that is re-read at every supervisor idle boundary or reconnect, consumed idempotently by request id, plus a Wedge config showing that the fire-and-forget version stalls under Drop.

### Re: network conditions in simulation, and fuzzing — opus-5-5 · 2026-10-05T00:30-04:00

> **Chat prompt (#chatprompt):** SHould the siumulation suite also account for various network conditions? Should we add fuzzing to proactively find new issues?

Yes to both. They do different jobs: the TLA+ models prove the *design* correct, while simulation and fuzzing find places where the *code* departs from that design.

**Simulation under network conditions:** `netadv4` already covers this. SimWorld (`src/sim_world.rs`) gets a seeded network layer with latency, jitter, reordering, drop, duplication and reconnect, including a Coder+Zscaler profile.

**Fuzzing:** two new phases (plan commit `48372a6948`):

- `netadv6`, **deterministic simulation fuzzing.** A seeded explorer drives SimWorld through random interleavings, network faults, edits, recycles, crashes and reconnects. After every step it checks invariant oracles, which are the same safety properties the TLA+ models state. A failing seed is shrunk to a minimal trace and committed as a permanent regression seed. `make check` runs a short budget plus every regression seed, and a nightly CI job runs fresh seeds for longer.
- `netadv7`, **coverage-guided fuzzing** (`cargo-fuzz`) of the bytes that cross a process or network boundary: IPC wire decode, markdown/patch parsers, frontmatter and CRDT op decode. The checks are no panics, and lossless round-trips where a parser claims them. `proptest` is used in only 3 files today; it extends to model-based tests where one transition table drives both the TLA+ model and the Rust test.

Deterministic simulation fuzzing is the one that would have found GH #136 proactively. The bug only shows up under one specific ordering of focus, recycle and stale-supervisor events, and random schedules reach orderings like that.

### Re: netadv batch dispatch — opus-5-5 · 2026-10-05T00:33-04:00

The `netadv1`–`netadv7` items were mirrored into the subagent queue. I dispatched the four that don't depend on anything, each with its own claim and its own worktree under `~/work/btakita/agent-doc-wt/`:

| Item | Owner | Scope |
|---|---|---|
| `#netadv1` | `subagent:netadv1` | channel-assumption audit of every TLA+/Lean model and IPC protocol, ranked by timing dependence, plus a list of one-shot-notification defects |
| `#netadv2` | `subagent:netadv2` | `NetChannel.tla` (deliver/drop/duplicate/reconnect, fair-lossy), a reference retransmit protocol, and 3 Wedge cfgs that must produce counterexamples |
| `#netadv4` | `subagent:netadv4` | SimWorld seeded network layer (`local` / `coder_zscaler` / `hostile` profiles), existing scenarios re-run under faults, and a fixed seed set added to `make check` |
| `#netadv7` | `subagent:netadv7` | `cargo-fuzz` targets for IPC decode, markdown/patch, frontmatter and CRDT decode, a corpus from redacted logs, and proptest model-based tests |

These three are held on dependencies, under coordinator claims with a 4h TTL so the loop doesn't spin on them:

- `#netadv3` needs `netadv2` (`NetChannel`) and the GH #136 model.
- `#netadv5` needs the round-trip counts from the `netadv1` audit.
- `#netadv6` needs the `netadv4` network layer.

Still in flight: GH #136. `#jbstagefallback` and the alt+shift menu fix are merged on `batch-0505`. The release stays held until GH #136 lands. If the network batch is ready by then, it ships in the same release; otherwise in the next one.

### Re: handoff to Codex before the weekly token limit — opus-5-5 · 2026-10-05T00:35-04:00

> **Chat prompt (#chatprompt):** We are almost at the week token limit. Make sure Codex can successfully pickup all subagents + tasks.

Everything in flight is now durable and visible to Codex:

- **All 5 running subagents checkpointed.** `gh136`, `netadv1`, `netadv2`, `netadv4` and `netadv7` each have a WIP commit plus a `HANDOFF.md` at the worktree root (goal, done, remaining, commands). They keep working and re-commit at each milestone, so whatever finishes before the limit is also saved.
- **Integration branch:** `batch-0505` already holds `#jbstagefallback` + the alt+shift menu fix (plugin 0.2.496, JB suite 714/0).
- **Handoff doc:** `tasks/agent-doc/handoff-2026-10-05-codex.md` (commit `247ee9b4d6`) has the item → worktree → state → dependency table, the merge order, the JetBrains version-conflict recipe, `make check` → one `make install` → release steps, and the network-correctness requirements from this session.
- **Codex skill refreshed** to 0.35.456 (it was at 0.35.455).
- **Queue entry point:** `#codexhandoff` is at the head of the queue and unclaimed, so Codex's preflight selects it first and it points at the handoff doc.

**The one trap:** the other heads are still claimed by this session (subagent claims have a ~2h TTL, coordinator claims ~4h), and claimed heads are invisible to preflight. Step 0 of the handoff tells Codex to check that no Claude subagent is still writing, then `agent-doc queue release` each claim it takes over. Without that, Codex would sit idle until the TTLs expire.

Both rules carry over: never two agents in one worktree, and never edit the main `src/agent-doc` checkout directly (another session has uncommitted edits there).

### Re: cont — batch integration + netadv3/5 dispatched — opus-5-5 · 2026-10-05T00:52-04:00

> **Chat prompt (#chatprompt):** cont

Continuing.

**Integrated into `batch-0505`:**
- `gh136` @ `9bb7edb2b`: `make check` green in its own worktree (11,027 passed, clippy clean, TLA green including the new `StaleColumnRecycle.tla` with 5 wedges).
- `netadv2` @ `6fdaa8607`: `NetChannel.tla` and `NetChannelRetransmit`, 4 wedges plus Reach.
- `netadv1` @ `fd64518b3`: the channel audit, 12 ranked risks R1–R12 and 23 send-once defects F1–F23.

The conflicts in `formal/tla/README.md` and `scripts/run_tla.sh` were additive on both sides, so I kept both (`StaleColumnRecycle NetChannelRetransmit` plus `libraries=(NetChannel)`). The branches' `HANDOFF.md` files were dropped. `make check` on the merged batch is running.

**GH #136 root cause:** the request to restart the stale supervisor was durable but expired. A later install fan-out request superseded it and lapsed after 15 minutes, so the stale supervisor saw no live request. Separately, stash promotion had no limit. The fix:
- requests caused by a replaced binary never expire;
- a request is "pending" or "overdue", measured from the first unconsumed request;
- a pure `stale_focus_admission` never widens the window, never promotes an overdue pane and never displaces a live turn;
- the layout receipt carries `gated_documents`, so the controller settles in one attempt instead of retrying forever.

Follow-ups the subagent left open:
- enforce `columns <= observed_panes` for every publisher;
- investigate the 60s focus settle and the gen-reset race on 1061.md;
- supervisors already stale on old code still need a one-time recycle;
- republish the layout after a recycle;
- bound stale panes outside the `stash` window.

**Now unblocked and dispatched**, each in its own worktree based on `batch-0505`:
- `#netadv3`: port the hot-path models (VisibleDeliveryReceipt, EditorReplicaStrand, RecycleSettleDispatch, AgentDocCloseout, PassiveTmuxSync, StaleColumnRecycle) to `NetChannel`, and fix the counterexamples, starting with the send-once defects F1–F23.
- `#netadv5`: stop timeouts being read as verdicts (R1 a 1s focus timeout kills a busy controller, R2 a slow attach drops the editor from the cut, R3 a slow connect deletes a live socket, R5, R6, R7, R8 duplicate prompt injection, R9 duplicate supervisor), plus tests that assert the round-trip count on hot paths.

`netadv4` and `netadv7` are still running. The handoff doc stays current at each step (latest `641939ecdf`), and every worktree keeps a committed `HANDOFF.md`, so Codex can resume at any point.

### Re: batch integrated, installed, pushed — opus-5-5 · 2026-10-05T01:09-04:00

> **Queue prompt:**
>
> do [#jbstagefallback]
>
> do [#netadv1]
>
> do [#netadv2]


> **Queue prompt:** The alt+shift agent-doc menu is not coming up.

Fixed and installed. Commit `ab3e68a3a` (GH #116, plugin 0.2.485) moved the Agent Doc Actions popup from Alt+Space to Ctrl+Shift+Alt+D in every keymap, which broke the key you had been using on Linux/X. Alt+Space is now back on the XWin keymap, alongside Ctrl+Shift+Alt+D. GNOME, KDE, Windows and macOS keep only Ctrl+Shift+Alt+D. Plugin 0.2.496 is installed. **Please check in the IDE:** in a `.md` editor, both Alt+Space and Ctrl+Shift+Alt+D should open the popup, and the XWin binding may need an IDE restart rather than a hot reload. If you meant some other key by "alt+shift", say so and I'll reopen this.

**Integrated into agent-doc `main` @ `8c3e97beb` (pushed):**
- `#jbstagefallback`: the unguarded `installAfterRestart` fallback is now refused and reports restart-required. **Done.**
- alt+shift (Alt+Space) popup fix, above.
- GH #136: the stale-supervisor recycle request never expires; a pure `stale_focus_admission` never widens, never promotes an overdue pane and never displaces a live turn; the layout receipt carries `gated_documents`; `StaleColumnRecycle.tla` has 5 wedges. The head stays claimed until the release, and then the issue closes (`#gh-fix` = fix then close on release). The fix's open follow-ups are now `#gh136follow`.
- `#netadv1` network channel audit, and `#netadv2` `NetChannel.tla`. **Done.**

**Verification:** `make check` on the merged branch exited 0, with nextest 11,027 passed / 265 skipped and every TLA+ model and broken variant behaving as required. `make install` from the tested worktree exited 0. I deliberately built from the worktree so the other session's uncommitted `agent-doc-start-runtime-io` edit in the main checkout stays out of the binary. The JetBrains plugin is 0.2.496. The library reload reached 0 of 1 editor endpoints because that endpoint was unavailable. The superproject submodule bump is `8ee08fd571`.

**Still running:** `netadv3` (models over NetChannel plus one-shot fixes F1–F23 plus SIM-F1/F2), `netadv5` (timeout-as-verdict fixes plus round-trip budgets plus SIM-F3), `netadv6` (simulation fuzzing) and `netadv7` (cargo-fuzz). `netadv4` is done but not merged yet; it goes in with the next batch. The release waits for that batch.

### Re: batch2 installed — sim-net + fuzzing; gh136follow dispatched — opus-5-5 · 2026-10-05T01:45-04:00

**`#netadv4`, done.** New crate `agent-doc-sim-net` with seeded profiles:
- `local`: no faults, byte-identical to before across 128 seeds;
- `coder_zscaler`: 20–180ms latency, jitter, 1% drop, 1.5–8s stalls;
- `hostile`: heavy reorder/drop/duplicate.

`make sim-net` (inside `make check`) runs 2048 schedules per profile. It found three ordering and duplication defects; all are being fixed:
- SIM-F1: supervisor lifecycle and heartbeat use last-arrival-wins, so a stale Ready can land after Busy and the next prompt is typed into a busy supervisor. Being fixed in `netadv3`.
- SIM-F2: queue pause/resume/drain have the same defect. Being fixed in `netadv3`.
- SIM-F3: dispatch requests have no idempotency key, so a resend injects a duplicate trigger. Being fixed in `netadv5`.

**`#netadv7`, done.** Five `cargo-fuzz` targets (IPC wire, markdown/patch, frontmatter, CRDT update, CRDT edits). Fuzzing found 7 real bugs, each fixed with a regression test and a minimized input:
- a template exchange-repair panic mid-write;
- 4 frontmatter round-trip bugs: body duplicated into the frontmatter, a duplicate key from a bare `\\r`, a preset losing its trailing newline, and `queue_active` dropped;
- a CRDT decode that requested about 100 GB of memory from a 9-byte input;
- a CRDT columnar counter overflow.

It also adds TLA-tied proptest model tests (SupervisorGenerationTransition, IpcBuildIdentity), corpus replay in `make check`, and an opt-in `make fuzz`.

**Integration:** `batch2` → agent-doc `main` @ `e8990627c` (pushed). `make check` exit 0, nextest 11,064 passed / 267 skipped. `make install` from the tested worktree exit 0. Superproject bump `d07c28d431`.

**Dispatched:** `#gh136follow` → `subagent:gh136follow`:
- (a) columns <= observed panes for every publisher;
- (b) the 60s focus settle and the `superseded` race;
- (c) a one-time recycle for supervisors already stale on old code, at a proven idle boundary only;
- (d) republish the layout when a recycle completes;
- (e) bound stale panes in non-stash windows.

**Still running:** `netadv3`, `netadv5`, `netadv6`, `gh136follow`. GH #136 and the release are held for the final batch. Handoff doc: `2e5ec57764`.
> **Chat prompt (#chatprompt):** also fix: tmux pane auto-sync not working when I navigate away from agent-doc-bugs.md to tsift.md or to lazily.md
>
> **Chat prompt (#chatprompt):** Fix in a subagent

### Re: tmux auto-sync and interrupted agent-doc batch — gpt-5 · 2026-10-05T06:13-04:00

> **Queue prompt:** Pick up from claude code sessions that were interrupted by weekly limit. Subagents were running.

Completed. I audited every interrupted Claude worktree, resumed the unfinished branches in delegated subagents, and integrated the completed work serially on the clean `batch3` worktree without touching the dirty main checkout.

The requested tmux navigation fix was implemented in a subagent and integrated as `668f02c1f`. JetBrains navigation events were reaching the controller, but a Lazily layout-effect panic terminated the detached worker while leaving its `active` lease latched. Later navigation to `tsift.md` or `lazily.md` only replaced the pending revision and could never spawn another worker. The worker is now unwind-fenced: it releases a failed revision, transfers a newer racing revision to exactly one replacement, and does not hot-retry the failed revision. Its focused suite passed 45/45, and it is covered by the combined release gate.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/136: Use formal logic to make the fix & the state transitions provably correct. Make sure the effects are provably reliable and have low latency.

Fixed, released, and closed. The integrated work bounds retained layout width under adversarial delivery, reapplies retained editor effects only to matching observations, fences lifecycle/focus traffic by receiver-local ordering, replaces stale supervisors only at a proven idle boundary, and republishes after recycle. `LayoutWidthBound` and the network-channel models include positive safety checks plus required wedge/reach counterexamples.

> **Queue prompt:** do [#netadv3]

Completed: failed focus and surface effects are retained and reapplied once to the next matching observation; stale layout/focus traffic is invalidated, and success/refusal/move/retirement clears the intent.

> **Queue prompt:** do [#netadv5]

Completed and integrated: correctness paths no longer treat slow transport as a definitive verdict, and hot-path delivery/dispatch behavior is bounded and idempotent.

> **Queue prompt:** do [#netadv6]

Completed: the deterministic simulator now models independent IO clocks, receiver-local lifecycle ordering, and rejected deliveries correctly. Fresh fuzzing ran 2,395 schedules × 160 steps without a finding; the release short budget and nightly path are wired.

> **Queue prompt:** do [#claimstrike]

Completed: an answered claimed free-text queue head now releases and strikes atomically instead of returning as completed residue.

> **Queue prompt:** do [#gh136follow]

Completed: all five GH #136 follow-ups landed, including the all-publisher width bound, superseded-generation handling, one-shot stale-supervisor replacement, post-recycle republication, and non-stash window bounds.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/137

Fixed, released, and closed. The dead test-only `response_heading` helper was removed; strict closeout canonicalization remains the production owner, and a real write/commit end-to-end test proves the attributed, timestamped heading reaches both the document and Git HEAD.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/138: use formal logic

Fixed, released, and closed. Definitive CRDT/native-save refusals now enter one shared durable editor-transport-health transition. Two failures degrade and arm recycle, three consecutive refusals unregister, proven delivery clears idempotently, and an already-applied write without a visible receipt records `ProjectionUnconverged`. The `EditorTransportHealth` positive, wedge, and unregister-reach models all passed.

> **Queue prompt:** release + publish

Published. The final release is [v0.35.458](https://github.com/btakita/agent-doc/releases/tag/v0.35.458) at `ef76f6d30`, with all Linux, musl, Windows, JetBrains, VS Code, and checksum assets. PyPI `agent-doc==0.35.458` is live and its clean-install verification passed. The installed binary reports 0.35.458; JetBrains 0.2.498 is byte-converged and VS Code 0.2.80 is installed.

> **Queue prompt:** do [#codexhandoff]

Completed. The interrupted-session audit, delegated resumptions, serial integration, verification, release, publication, and issue closure are all finished.

Verification on the exact release tree:

- `make check`: exit 0; 11,133/11,133 native tests passed across 188 binaries, 270 skipped.
- TLA+: every positive model passed and all 83 required wedge/reach witnesses fired.
- `make tmux-ci`: exit 0; 33 live/ignored integration tests passed.
- GitHub Release, PyPI, Pages, Book, and CI run `37289728900` all completed successfully; CI independently passed both `make check` and `make tmux-ci`.

The exchange is large, but the active queue was drained rather than stalled for compaction, as required.
<!-- agent:boundary:3d7316c5 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#subagents #spec-test-build-install-commit-push" priority go -->
<!-- /agent:queue -->


<!-- agent:backlog priority -->
6local-splice-batch 3 splices 34346->34137 chars, then 00:40:03 exact inverse back to 25509a1e (undo/redo or plugin-internal flip; no VFS reload/projection logged). Blocked on operator: which text was lost + when (JetBrains Local History on api.md). Then trace the dropping path and fix with regression coverage.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#qsubattr5] [agent-doc] Queue subagents attribute, phase 5 (plan: tasks/agent-doc/plan-queue-subagents-attribute.md): dogfood in tasks/software/tsift.md (replace per-line #subagent: prefixes with subagents=3); verify via queue claims + ops.log that each head is dispatched exactly once and none drain inline.
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
