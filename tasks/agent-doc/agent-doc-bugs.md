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
<!-- agent:boundary:da65c2e1 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#subagents #spec-test-build-install-commit-push" priority -->
<!-- /agent:queue -->


<!-- agent:backlog priority -->
- [ ] [#gh131nonipc] [agent-doc] Follow-up to GH 131: some of the issue's failures (socket_visible_write divergence refusal, serialized_atomic_write retention) never reach the IPC send, so they record nothing in editor_transport_health and cannot drive degraded/recycle. Route those retention outcomes into the same typed transport-health recorder, and consider treating an endpoint as unregistered after N consecutive rejections (issue suggestion, not implemented in 2bedfb560).
- [ ] [#gh133sigterm] [agent-doc] Follow-up to GH 133: a deliberately SIGTERM'd supervisor in a VISIBLE column is still respawned by the controller watchdog, because there is no termination handler to distinguish a deliberate kill from a crash. Add a SIGTERM handler that records an intentional-exit marker the watchdog honours, without breaking the existing replacement path (SIGTERM then cold-start). Also decide: should idle owned supervisors whose pane is only stashed be reaped? specs/supervisor.md currently says no.
- [ ] [#jbstagebackup] [agent-doc][jetbrains] Follow-up to jbpluginvanish: the Java upgrader's restart staging (JetBrainsPluginUpgradeAction.stageForRestart) must still emit delete-then-unzip for the IDE's action.script, so a staging whose zip vanishes (or a full disk at restart) before any agent-doc install runs can still remove the plugin. Close the gap: have preflight/supervisor auto-purge doomed stagings (purge_pending_stagings_for StagingPurge::Doomed in agent-doc-fs/src/jetbrains_install.rs) instead of only warning, and/or stage a backup copy the restart can restore. Also clear the stale fixed-name agent-doc-jetbrains-0.2.469.zip left in ~/.cache/JetBrains/IntelliJIdea2026.1/plugins/.
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
