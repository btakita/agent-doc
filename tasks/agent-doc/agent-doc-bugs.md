---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: 6bd4c579-6308-4846-b5b6-af24bb0c65f8
  codex: 01a17616-9ddc-7ee0-86df-a42c74605356
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
presets:
  '#agent-doc-causes': 'Fix agent-doc causes'
  '#audit': 'Audit agent-doc runtime for dogfooding. Add issues to the backlog + queue.'
  '#fix-dataflow': 'Fix any issue you see at multiple levels of the dataflow.'
  '#repo': 'https://github.com/btakita/agent-doc'
  '#self': 'agent-doc-bugs.md'
dogfood:
  path: tasks/agent-doc/agent-doc-bugs.md
  auto: true
queue: go
---

## Status <!-- agent:status patch=replace sync="##sync-release" -->

📤 Queue handoff: 1 reported item awaiting coordinator integration, verification, and closeout.
Main is at `b6b9af6e15` and agent-doc 0.35.532 is installed locally. Release v0.35.532 was published from `71f9801b49`. Post-release main now includes the inline-code-safe, occurrence-ordered queue preset authorization fix for GitHub issue #331 and the positive-evidence-only Remote Dev pane-order fix for GitHub issue #332; both issues are closed, the integrated full suite passed, and the current mainline was installed locally.

<!-- status:sync -->
Released and published agent-doc v0.35.556 from release commit `5cbfdbbc4c`; tag `v0.35.556` is the stable latest release (GitHub Latest, PyPI 0.35.556, stable target channels). JetBrains classic/262/263 0.2.839/0.2.841/0.2.840, VS Code 0.2.83. It adds onboarding Getting started, the queue marker legend, the component-marker fences (live edit, cold start, JetBrains edit-time warning), row-aware editor attach, reliable echoed-head strikes beside a loop queue, doctor/autofix exit codes, and pane-process launches for start/route/fan-out. GH #378 closed; GH #379 stays open pending Remote Dev proof (todo `#gh379-remotedev-proof`). Post-release `origin/main` is `0b226ee6d5` (unreleased, installed locally): the in-IDE restart-required prompt (JetBrains 0.2.842/0.2.844/0.2.843), no-argument `/agent-doc` on all four harnesses via `agent-doc document`, `agent-doc queue repair-stale-completion`, id-less `agent:run` / `agent:loop` named by kind (`run` / `loop`), the GH #376 measured route now publishing after its own surface Sync, and a test + spec pinning that the done/backlog free-text strike skips claimed rows.
<!-- /status:sync -->

Queue markers: 📥 pending · 🟡 claimed · 🚧 running · ☑️ reported · ⚠️ lost · 🔄 needs reconcile · ❔ unknown · ⏭️ skipped · ⏳ awaiting operator · 🙋 operator action · 📌 pinned · 📍 agent pin ([legend](https://github.com/btakita/agent-doc/blob/main/docs/guide/queue-markers.md#queue-markers))
<!-- /agent:status -->
 

## Exchange <!-- agent:exchange patch=append subagents -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261026-215049.md`*

Compacted content:
- Archived 49 response topic(s): Subagent completed: `subagent:grok_queue_ui` returned work for queue item “In sdk-demo.md using Grok, the queue item ...; Subagent completed: `subagent:lazily_block_reason` returned work for queue item “On lazily.md, `- ⚠️ [#subagent:corec...; Subagent completed: `subagent:tmux_auto_sync` returned work for queue item “tmux pane auto sync is not working. Sync ...; 46 more
- Prior summary/context: prior compacted content: Archived 7 response topic(s): JetBrains Marketplace registration; JetBrains CLI installer an...

### Re: session resumed after /clear — opus-5 · 2026-10-26T18:10-04:00

**Previous session's workers:** all ten finished before the clear, and their work is on `main`. Nothing was in flight, so nothing needed adopting.

**Dispatched this cycle (2), both 🚧 running in their own worktrees under `~/work/btakita/agent-doc-wt/`:**

- `do [#decisionbacklogstrike]` → `subagent:decisionbacklogstrike` (`analysis/decision-backlog-strike`). This is an analysis, not a change. It covers what a backlog strike does today, what closing claimed free-text rows would do to the claim, the worker, its unintegrated report and any request packet, the hazards ranked, safer variants such as striking only ☑️ rows or only `do [#id]` heads, and a recommendation. It will not implement "yes", because that's your call.
- `/agent-doc` with no arguments → `subagent:noargparity` (`feat/skill-no-arg-parity`). It builds on the earlier `noargskill` worker's unintegrated commit `81c3de55d5`, which already resolves the pane's claimed document. The worker rebases it onto `main`, which it is 26 commits behind, then checks each harness (Claude Code `/agent-doc`, a plain Codex `agent-doc` message, Grok Build, OpenCode) to confirm the bare form gets the same admission, contract, and closeout as the `<FILE>` form, and closes any gaps.

**`#fixautoupgrade`:** the 17:39 response before compaction answered your questions (re-qualify safety, re-qualify vs GH #339, and the serialization cost of Shrink). It's in the archive. The row stays Deferred until you pick what to queue. My recommendation is unchanged: keep the restart prompt as the fallback (done), start the GH #339 shell, and keep moving `TerminalUtil`/`PatchWatcher` behind the native library. Moving `CrdtReplicaManager` should wait for a per-keystroke latency benchmark.

**Leftover claims:** `autoupgrade`, `cellrepair` and `noargskill` are ☑️ claims owned by the cleared session. Their work is done and is waiting on your decisions, so I left them alone. Releasing them, as preflight suggested, would dispatch finished work again.

**Decisions for you:**
- Command names: `agent-doc queue repair-stale-completion` (cellrepair) and `agent-doc resolve-pane-document`, used by the no-arg work. The no-arg worker keeps the current name and will suggest alternatives.
- `#fixautoupgrade`: which follow-up to queue.
- Restart IntelliJ IDEA 2026.3 to load plugin 0.2.843.

### Re: do [#decisionbacklogstrike] — opus-5 · 2026-10-26T18:29-04:00

> **Queue prompt:** do [#decisionbacklogstrike]

**Decision (yours): keep skipping.** The "already done / already tracked" free-text strike keeps leaving claimed rows (🟡 / 🚧 / ☑️) alone. On `main` as `30ff9a7d66` and installed. The commit adds a test (`backlog_strike_leaves_claim_projected_free_text_rows_and_their_evidence`) and a spec paragraph (`#qftbklgclaimed`) that lock this in, so it can't change by accident. `make check-fast`: 3749 run, 3749 passed, 28 skipped.

**Why "yes" was the wrong trade:**

- **☑️ rows lose the worker's report.** The strike removes the claim, the receipt, and the completion cell in `agent:exchange` that holds the report. Nothing left in the document says "integrate this branch". These rows often wait hours.
- **A repeated request can be struck.** Free-text rows match `agent:done` by word overlap. A new request that restates old work would be closed against the old record. Id rows have repeat protection; free-text rows don't.
- **A running worker gets hidden.** For 🟡 / 🚧 rows the claim survives, but the row disappears from view. In the worker's probe the expected `❔` reopen never fired, and the worker's later report would leave a stale cell behind.
- **Weaker proof than closeout.** Closeout strikes a claimed row only on an exact `> **Queue prompt:**` echo. A word-overlap match is a weaker test for the same row.

**Today's skip is an accident, not a rule.** The candidate reader takes the first `#` in the row as its id, so the owner tag `[#subagent:x#1]` reads as an id. Rows that mention "GH #361" are skipped for the same reason, and a claimed row whose ☑️ hasn't been painted yet can still be struck. A sturdier version would read the claim ledger instead. It's not queued; ask if you want it.

**`/agent-doc` with no arguments:** the worker is done on all four harnesses. Grok's `agent_doc_admit` was the one real gap: it now admits the pane's claimed document when no `file` is given. You named the lookup command **`agent-doc document`**, and the worker is renaming it from `resolve-pane-document` now. I'll integrate it when it reports.

**Still waiting on you:** a name for `agent-doc queue repair-stale-completion` (`cellrepair`), which follow-up to queue for `#fixautoupgrade`, and an IntelliJ IDEA 2026.3 restart.

### Re: id-less run + loop, no-arg entrypoint, stale-cell repair — opus-5 · 2026-10-26T19:00-04:00

> **Queue prompt:** I removed id from run + loop. Ensure these items are distinct and have semantic meaning.

On `main` as `a4c8f2cf99` (`#idlessqueueidentity`) and installed. Removing the ids broke this document's queues: `queue schedule` refused ("requires a nonempty explicit `id=`"), and `queue claim`/`brief` found no live heads, so nothing could be dispatched.

**The rule now:**
- An explicit `id=` always wins.
- A lone queue with no id stays `default`, as before.
- In a multi-queue document, an id-less queue that is the only one of its kind is named by its kind: `## Run` is `run`, `## Loop` is `loop`. Heading text and position never count, so renaming or reordering them doesn't move any state. The derived id is never written back, so your markers stay id-free.
- Two id-less queues of one kind, or a kind name already used as another queue's `id=`, fail closed with a message that names both markers and the fix.

**Existing state carried over:**
- Claims are keyed by row text, so they were unaffected.
- The hourly loop's schedule moved from `issues` to `loop` with its history intact: 5 passes run, pass 6 open.
- Briefs now say `Origin queue id: run`.

**Verification:**
- Tests use a fixture shaped like this document, with and without the old ids. With the fix disabled, all 9 key tests fail.
- The integrated `make check-fast` passed: 11137 run, 11137 passed.

> **Queue prompt:** `/agent-doc` without any arguments should be the same as `/agent-doc <document-path>`. Apply the equivalent to all agents Claude Code + Codex (`agent-doc <document-path>`, Grok Build, Opencode).

> **Queue prompt:** In the agent terminal, running the `agent-doc` skill (`/agent-doc` on Claude + Grok + Opencode and `agent-doc` in Codex) without any arguments should run agent-doc on the claimed file for the tmux pane.

Both rows ask for the same thing, and both are done: on `main` (`6663ec15cc`, `54a2c3f14e`, `6750e04043`) and installed. A bare entrypoint now runs the normal cycle on the document this tmux pane has claimed:

- **Claude Code** `/agent-doc` and **Codex** plain `agent-doc`: the hook rewrites the bare form into the `<FILE>` form before admission. A test runs both forms side by side and checks the contract, session binding, and Stop-hook decision are identical.
- **Grok Build**: this was the real gap. `agent_doc_admit` with no `file` now admits the pane's claimed document.
- **OpenCode**: it has no admission hook, so its skill runs `agent-doc document` first.
- The lookup command uses the name you picked, **`agent-doc document`**. Outside tmux, in an unclaimed pane, or when a pane claims several documents, it fails closed and names the reason.
- **Known issue:** in a directory with no `.agent-doc` of its own, the lookup walks up to `$HOME` because `~/.agent-doc/` exists. It still refuses safely, but the refusal names the wrong root. Not queued.

> **Queue prompt:** one-time repair: retire stale subagent completion cells and report receipts left behind by strikes made before the completion-cell retirement fix, or made by hand in the editor, in this document and other session documents (found by the completion-cell retirement fix, 2026-10-26)

`agent-doc queue repair-stale-completion` (the name you kept) is on `main` as `8dc06a9f22` and installed. It is dry-run first; `--apply` changes the document, and `--project-root` scans every session document.

- **This document:** already clean, because the compaction removed its leftovers.
- **`tasks/workstation/upgrade.ad.md`:** 7 boilerplate cells retired. I checked that every other byte is unchanged. agent-doc refused to commit it, because that document already had older uncommitted edits (the `queue: go` frontmatter line removed, and a `(HEAD)` marker). Its next `/agent-doc` cycle commits the repair together with them.
- **`tasks/software/lazily.md`:** 14 stale cells, but its session was busy (pane %7, 3 workers). I queued it as a follow-up row right below this one.

> **Queue prompt:** Why have the ☑️ run items not completed?

A ☑️ row means the worker reported, but the row isn't struck until the coordinator integrates the work and closes it. These three were finished but held at that step:

- **`noargskill`** and **`cellrepair`** were waiting on their command names. Both are now merged and closed above.
- **`#fixautoupgrade`** is waiting on your direction. The restart-prompt fallback is merged; the 17:39 answer (in the compaction archive) covers re-qualifying vs GH #339 and the performance cost of "Shrink". It stays Deferred until you choose what to queue.

The previous session ended (`/clear`) while all three sat there, so nothing was driving them. That's also why preflight called their claims stale.

**Also running:** a worker is fixing the cause of a timing race in `gh376_zero_column_route_awaits_the_measured_columns_that_land_after_it`. Under load, the editor-surface publisher sometimes wins the pane-layout publish over the route. The test is already on main and isn't caused by today's changes. I'll integrate the fix when it reports.

**Still waiting on you:** the `#fixautoupgrade` direction, and an IntelliJ IDEA 2026.3 restart to load plugin 0.2.843.

### Re: do [#closehttpsgithub] — opus-5 · 2026-10-26T19:10-04:00

> **Queue prompt:** do [#closehttpsgithub]

**Not yet. You chose to keep it open until there's proof.** [GH #379](https://github.com/btakita/agent-doc/issues/379) is still open, and its only comment is our 19:12Z triage. The reporter ran CLI 0.35.553 with JetBrains plugin 0.2.827/0.2.826, both older than the fixes: #373 shipped in 0.35.554, and #376 in 0.35.555 with plugin 0.2.835. No one has confirmed since upgrading.

**On this machine the fix works, but that doesn't cover the reported setup:**

- Since the fixed builds were installed, Run Agent Doc routes end with exit 0 and the layout converges.
- `editor route requires the exact visible layout before dispatch` never appears in `ops.log`.
- The one failed route (22:30Z) had an unrelated cause: the pane was still showing a permission prompt.
- The report came from Remote Dev with a Windows JetBrains Client (`remote_client_selected_files`, `flat_evidence_has_no_column_geometry`). Nothing on this machine exercises that path.

**What closes it** (added as todo `#gh379-remotedev-proof`): on that Remote Dev backend, `agent-doc --version` is 0.35.555 or newer and the 2026.3 plugin is 0.2.835 or newer. Then Run Agent Doc should end with exit 0, and `ops.log` should show `controller_editor_route_measured_layout_resolved` without the error. When that passes, tell me and I'll close the issue with the evidence.

**Still running:** the fix for the `gh376_…` pane-layout timing race in the test.

### Re: GH #376 route/surface publish race fixed — opus-5 · 2026-10-26T19:15-04:00

**The `gh376_…` test failure was a real ordering bug, not a test problem.** The fix is on `main` as `0b226ee6d5` and installed. No IDE restart is needed.

**What went wrong:** when the measured-columns forward arrived, it woke the waiting Run Agent Doc route *before* it published its own surface sync. Under load, the route could publish first. The surface sync then landed as a newer generation (`focus: None`, `publisher: EditorSurface`) and wiped out the route's focus on the document you ran. The spec says the measurement answers the route the same way a surface reference does, so the route has to publish last.

**The fix:** the surface-observe handler marks its publication as in flight until its own sync has published. The route's measured lookup waits for that, within its own `--wait-for-ready` budget. The route's layout is now always the newest generation.

**Proof:**
- Two new tests use test-only hooks to force each order, and both pass.
- With the fix disabled (cold build), the route-woken-first test fails with the reported `left: EditorSurface right: Route`.
- The integrated `make check-fast` passed: 4270 run, 4270 passed.
- The worker's earlier failed runs came from its own changes (clippy, and a TLA conformance name). It never reran a flaky test to get a pass.

**Dispatched:** the `lazily.md` stale-cell repair row → `subagent:lazilyrepair` (🚧). It polls until the `lazily.md` session is idle, for up to 45 minutes. Then it runs the dry run and `--apply`, checks that only completion cells changed, and commits. If the other session never goes idle, it changes nothing.
<!-- agent:boundary:3ec64169 -->
<!-- /agent:exchange -->

<!--
-->

## Run <!-- agent:run subagents preset="#spec-test-build-install-commit-push" priority go -->
- ☑️ [#subagent:autoupgrade#1] do [#fixautoupgrade]: I like **Keep restart-only + prompt** as a fallback solution. We should attempt to auto-upgrade though. **Build the GH #339 architecture** seems like a reasonable solution sfor most cases. Not sure what **Re-qualify on 263.7307.44** would imply. Is **Re-qualify on 263.7307.44** safe and how effective would it be? Would **Re-qualify on 263.7307.44** be more likely to successfully run auto-upgrade than **Build the GH #339 architecture**? Your recommendation of **Keep restart-only + prompt** + **Shrink the restart count** seems reasonable if it performs well.
- 🚧 [#subagent:lazilyrepair#1] When the tasks/software/lazily.md session is idle, run `agent-doc queue repair-stale-completion tasks/software/lazily.md --apply` (dry run: 14 stale completion cells; skipped 2026-10-26 because that session was busy) and commit it
<!-- /agent:run -->

## Loop <!-- agent:loop interval="1h" priority -->
- 📥 Watch https://github.com/btakita/agent-doc/issues for new issues. Add new issues into the backlog + queue except issue #40 + #45.
<!-- /agent:loop -->

## Todo <!-- agent:todo priority -->
- [ ] [#ahfe] Live-check GH #267 on the 263 IDE: after `agent-doc upgrade` WITHOUT restart, once idea.log shows `[native] hot-reloaded libagent_doc`, Run Agent Doc should complete (ops.log `editor_route_superseded_editor_admitted`, route output ends with the advisory line). 2026-10-15 operator: did not work (believed); watch for bug reports before reopening GH #267.
- [ ] [#ctg7] Under heavy load, switch the editor 2 panes -> 1 -> 2 and confirm tmux follows and stays at 2 (stale-controller fix).
- [ ] [#coder-remote-install-02805] Repair Coder Remote Dev sync after the two-sided install: open /home/brian/work/btakita/agent-loop as the backend IDE project root; ensure Client/Gateway and backend both load JetBrains 263 plugin 0.2.824; restart both; on the backend run agent-doc env --json, agent-doc tmux ensure tasks/agent-doc/agent-doc-bugs.md --ide-terminal --json, and agent-doc session status tasks/agent-doc/agent-doc-bugs.md; require client idea.log to show snapshot schema negotiated with geometry=measured, backend idea.log to show accepted ingress with no DELIVERY_FAILED, and .agent-doc/logs/ops.log to show controller_editor_surface_observed client=jetbrains-rd@... surface=remote-client:... followed by pane_layout_desired_published with column_order=editor. If DELIVERY_FAILED No such file or directory persists, fix ControllerSurfaceSnapshotPublisher to resolve the controller from each session document root instead of an unrelated IDE project basePath, expose connected-without-accepted-publication as blocked while preserving the no-guessing fence, and add a regression test.
- [ ] [#jb263-feed-verify] In IntelliJ IDEA 2026.3, configure only https://github.com/btakita/agent-doc/releases/latest/download/agent-doc-jetbrains-263.xml, refresh Plugins, update Agent Doc from 0.2.816 to 0.2.824 or newer, restart the IDE, and verify the “Cannot update plugin” warning clears; if it does not, capture the configured repository entry and relevant idea.log update-check lines.
- [ ] [#xj75] Choose the JetBrains restart-free upgrade direction in the [#subagent-operator-request-autoupgrade-263-path] packet in agent:exchange: keep restart-only + prompt, re-qualify on IU 263.7307.44, build the GH #339 shell, or shrink the restart count
- [ ] [#subagent-operator-request-autoupgrade-263-path] Answer the blocking subagent question for tracked work jb262dynlauncherrecommen for queue item ‘Fix the auto-upgrade not working...the IDE needed to be restarted.’ in agent:exchange: The exact-263 package is restart-only by spec (GH #339 closed with that contract; `#jb262dynlauncherrecommen` is the same open decision for 262). (choices: Keep restart-only + prompt, Re-qualify on 263.7307.44, Build the GH #339 architecture, Shrink the restart count) (source: agent:review [#jb262dynlauncherrecommen])
- [ ] [#gh379-remotedev-proof] Close GH #379 (https://github.com/btakita/agent-doc/issues/379) after proof from its Remote Dev setup (Windows JetBrains Client): on that backend, `agent-doc --version` is 0.35.555 or newer and the IntelliJ IDEA 2026.3 plugin is 0.2.835 or newer; Run Agent Doc then ends exit 0 and ops.log shows `controller_editor_route_measured_layout_resolved` with no `editor route requires the exact visible layout before dispatch`.
<!-- /agent:todo -->

## Backlog <!-- agent:backlog priority queue -->
- [ ] ☑️ [#fixautoupgrade] 🙋☑️ [#subagent:autoupgrade#1] Fix the auto-upgrade not working...the IDE needed to be restarted. [#subagent-operator-request-autoupgrade-263-path]: would **Shrink the restart count** have performance issues due to extra serialization?
<!-- /agent:backlog -->

## Review <!-- agent:review -->
- [/] [#jetbrainsmarketplaceprofile] [marketplace-approval] [agent-doc] Plugin 35436 is hidden and under initial JetBrains moderation; after approval, replace the basic description with polished copy, add Getting Started, Documentation, and Bugtracker URLs, then verify the public listing.
- [/] [#jb262dynlauncherrecommen] [recommended] [operator-verify] [agent-doc] Operator decision: jb262dynupgrade (153c72916) chose the accurate-warning option (a 262 package reports declined_by=modular_package and falls back to file replacement + restart). Either keep that as the supported policy (then close this), or authorize a real restart-free launcher for the exact-262 split-mode plugin that swaps backend + frontend (the frontend may run in a separate JetBrains Client process), proven against a live 262 IDE.
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically by a human reply on pypi/support#12214, or make pypi-quota-check reporting a ceiling in reach. Rechecked 2026-10-12: pre-cutover 131 releases 8.361 GiB (0.35.113..0.35.343), post-cutover 91 releases 3.19 MiB (mean 35.9 KiB), about 47,722 further releases fit; VERDICT headroom fine. The support issue is still open with one comment, unchanged since 2026-09-10. Nothing to act on or delete. If the grant lands, record it and drop this item; deletion stays reserved for a ceiling the authenticated check reports.
- [/] [#gh299-live-263-proof] Live-verify v0.35.519 on JetBrains 263.7031 with changed plugin sources: `agent-doc upgrade` must hot-reload and complete without an IDE restart; close GH #299 only with that proof.
<!-- /agent:review -->

## Icebox <!-- agent:icebox -->
- [ ] [#ghrecvpolicy] [agent-doc] Recipient-side [receive] auto_create_components policy for multi-user workspaces (GH #40, https://github.com/btakita/agent-doc/issues/40). Parked until multi-user workspace support exists.
<!-- /agent:icebox -->

## Done <!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
