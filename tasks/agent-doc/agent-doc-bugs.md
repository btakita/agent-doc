---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: 5c496cf2-d923-4cdb-bff1-73ad7c75c246
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

Main is at `b6b9af6e15` and agent-doc 0.35.532 is installed locally. Release v0.35.532 was published from `71f9801b49`. Post-release main now includes the inline-code-safe, occurrence-ordered queue preset authorization fix for GitHub issue #331 and the positive-evidence-only Remote Dev pane-order fix for GitHub issue #332; both issues are closed, the integrated full suite passed, and the current mainline was installed locally.

<!-- status:sync -->
Released and published agent-doc v0.35.556 from release commit `5cbfdbbc4c`; tag `v0.35.556` is the stable latest release (GitHub Latest, PyPI 0.35.556, stable target channels). JetBrains classic/262/263 0.2.839/0.2.841/0.2.840, VS Code 0.2.83. GH #378, #380 and #381 closed; GH #379 stays open pending Remote Dev proof (todo `#gh379-remotedev-proof`). Post-release `origin/main` is `4e17a4c793` (unreleased, installed locally; JetBrains classic/262/263 0.2.862/0.2.863/0.2.864 need an IDE restart, VS Code 0.2.89, Zed 0.1.2): the in-IDE restart-required prompt, no-argument `/agent-doc` via `agent-doc document`, `agent-doc queue repair-stale-completion`, id-less `agent:run` / `agent:loop` named by kind, the GH #376 measured-route ordering fix, the GH #339 shell design with stage R1 complete and the adopted Exact-263 reopen rule, route-outcome / exchange-prefix-repair / session-command-outcome moved into the hot-reloaded native library (JetBrains and VS Code), an edit-proportional native replica flush that also folds remote ops incrementally plus a windowed Kotlin flush-prepare step, the JetBrains editor-tree focus listener deferred until the split tree exists, todo `id=` always winning, the editor frontend/backend parity matrix, VS Code stale-VSIX prompt + measured layout + workspace extensionKind + activation receipt + code-server/Remote-SSH install, the VS Code koffi owned-string free crash fix and koffi shipped in the VSIX for 7 platforms, Zed registration identity + in-place binary handoff + document-only routing, a PreToolUse guard against agents killing the live tmux server, the GH #380 supervisor harness-binary install-dir fallback, GH #381 project-preset queue heads draining as presets, and queue claims that end on a strike and never follow across queues (`#claimfollowstruck`), the PreToolUse tmux guard in every harness user layer, the same install-dir fallback for headless harness launches (`#headlessharnesspath`), claim closure judged across every queue (`#claimclosureallqueues`), no actor write after a provisioning claim launches its start (`#claimpanecasrace`), a hermetic koffi round-trip test (`#vscodenativeceiling`), hermetic codex command-builder tests for CI runners without codex, fenced post-launch route rebinds that never overwrite the owner's actor record (`#dispatchrebindaudit`), a status-sync `agent-doc commit` that lands beside pending subagent completion cells (`#statussynccommitrefusal`), pre-dispatch rebinds that never write Ready over a running owner (`#predispatchreadyoverwrite`), sync binding writes fenced on generation plus actor record revision (`#syncbindingfence`), revision-fenced controller stale closes and prune (`#controllerclosefence`) and route lifecycle writes (`#bindingcallersaudit`), a supervisor stale-Busy self-heal that cannot wedge an admin drain (`#drainbusymask`), and hermetic resync prune tests (`#prunetestisolation`).
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

### Re: lazily.md stale-cell repair — opus-5 · 2026-10-26T19:57-04:00

> **Queue prompt:** When the tasks/software/lazily.md session is idle, run `agent-doc queue repair-stale-completion tasks/software/lazily.md --apply` (dry run: 14 stale completion cells; skipped 2026-10-26 because that session was busy) and commit it

**Not run. Nothing changed.** `subagent:lazilyrepair` polled for 45 minutes (19:15 to 19:57) and the `lazily.md` session never went idle:

- **Pane %7:** `alive-busy` until 19:45, then `alive-unobservable` with `prompt_ready=unknown`. It is still that way now, with agent-doc still running in the pane.
- **The file:** `tasks/software/lazily.md` was last committed 21 hours ago and has uncommitted edits. Those are most likely your unsent edits waiting for that session's next turn. agent-doc refuses to commit a repair around unanswered edits, and that's correct, so running `--apply` now would only leave the repair uncommitted next to them.

**Moved to todo `#lazily-stale-cell-repair`** so the queue stops re-dispatching it. Run it after the next `lazily.md` cycle commits that document clean.

**Worth a look:** pane %7 shows `owned_pane_ready_busy_conflict`. The actor and controller lease say `ready`, but the live pane says busy and is now unobservable. If no real turn is running there, `agent-doc session clear` from another pane is the documented recovery. I didn't run it, because it would interrupt a session I don't own.

### Re: is #fixautoupgrade done, and #jb263-feed-verify — opus-5 · 2026-10-26T20:21-04:00

> **Queue prompt:** do [#fixautoupgrade]: I like **Keep restart-only + prompt** as a fallback solution. We should attempt to auto-upgrade though. **Build the GH #339 architecture** seems like a reasonable solution sfor most cases. Not sure what **Re-qualify on 263.7307.44** would imply. Is **Re-qualify on 263.7307.44** safe and how effective would it be? Would **Re-qualify on 263.7307.44** be more likely to successfully run auto-upgrade than **Build the GH #339 architecture**? Your recommendation of **Keep restart-only + prompt** + **Shrink the restart count** seems reasonable if it performs well.

> **Operator note:** I like **Keep restart-only + prompt** as a fallback solution. We should attempt to auto-upgrade though. **Build the GH #339 architecture** seems like a reasonable solution sfor most cases. Not sure what **Re-qualify on 263.7307.44** would imply. Is **Re-qualify on 263.7307.44** safe and how effective would it be? Would **Re-qualify on 263.7307.44** be more likely to successfully run auto-upgrade than **Build the GH #339 architecture**? Your recommendation of **Keep restart-only + prompt** + **Shrink the restart count** seems reasonable if it performs well.

**Your plan, as built:**

- **Keep restart-only + prompt as the fallback: done.** It's live in your IDE (0.2.843).
- **Attempt real auto-upgrade with the GH #339 architecture:** queued as `#jb339shelldesign`.
- **Shrink the restart count, if it performs well:** queued as `#jbshrinkrestarts`, and each move is gated on a per-keystroke latency benchmark so it only lands if it doesn't regress.
- **Re-qualify on 263.7307.44: not queued.** It's low risk, but the worst case leaves Agent Doc half-unloaded until you restart. Even a pass would only cover local IDEs on that one build, not Remote Dev, and any later plugin change could quietly undo it. GH #339 is far more likely to give you auto-upgrade that keeps working. Ask if you still want the quick experiment.

> **Queue prompt:** do [#cazn]

**`#fixautoupgrade` is closed, as you chose, and the next steps are in the backlog.** "Done" here means the fallback, not restart-free auto-upgrade:

- **Done and running:** the restart-required prompt. Your IntelliJ IDEA 2026.3 (IU-263.7307.44) loaded Agent Doc 0.2.843 at 18:07. Any upgrade that still needs a restart now shows a sticky "Agent Doc Plugin Update" balloon with **Restart IDE**.
- **Not done:** restart-free upgrades. [GH #339](https://github.com/btakita/agent-doc/issues/339) closed on 2026-10-21 after concluding that today's 2026.3 plugin layout has no safe restart-free path. The fix is a new architecture, and the two new backlog items start it:
  - `#jb339shelldesign`: design a stable shell plugin that loads the real code in its own classloader, so an upgrade swaps the code without IntelliJ unloading anything. It also covers activation receipts for the Remote Dev frontend. The deliverable is a spec plus a proof-of-concept spike.
  - `#jbshrinkrestarts`: move `TerminalUtil` and `PatchWatcher` behind the native library, which already reloads without a restart, gated on the latency benchmark. `CrdtReplicaManager` stays in Kotlin unless the boundary carries binary op deltas.
- I closed todos `#xj75` and `#subagent-operator-request-autoupgrade-263-path` too, since this was the direction they asked you to choose.

> **Queue prompt:** do [#jb263-feed-verify]

**Verified from the IDE's own files, and the todo is done:**

- **Only the feed is configured.** `pluginUpdateSources.xml` lists exactly `https://github.com/btakita/agent-doc/releases/latest/download/agent-doc-jetbrains-263.xml`.
- **The feed is valid for your build.** It serves 0.2.840 (from v0.35.556) with `since-build="263" until-build="263.*"`, which covers IU-263.7307.44.
- **The plugin is past the target.** 0.2.843 is loaded, well past the todo's 0.2.824 target, after your restart.
- **The warning is gone.** The current `idea.log` has zero "Cannot update plugin" lines.

**One caveat:** 0.2.843 came from `make install`, not from the feed, and the feed's 0.2.840 is older. So the IDE has nothing to update through the feed right now. The feed-driven update path gets its first real exercise when the next release publishes a newer 263 package.

**Your IDE restart is done**, so I've dropped it from my list of asks.

### Re: autoupgrade follow-ups dispatched — opus-5 · 2026-10-26T20:23-04:00

**Dispatched (2), both 🚧 running in their own worktrees under `~/work/btakita/agent-doc-wt/`:**

- `do [#jb339shelldesign]` → `subagent:jb339shell` (`fix/jb339-shell-design`).
  - **Deliverables:** a spec section for the shell-plugin and child-classloader architecture, plus an isolated proof-of-concept spike. The spike loads a versioned implementation, lets the shell own the registrations, swaps generations, and shows the old classloader can be garbage-collected.
  - **Not shipped:** nothing users install changes.
  - **Will ask you first:** any choice that gives up Remote Dev support or adds a separately installed shell plugin.
- `do [#jbshrinkrestarts]` → `subagent:jbshrink` (`fix/jb-shrink-restarts`).
  - **Benchmark first:** it adds a per-keystroke latency benchmark on a copied ~120 KB document and records a baseline.
  - **Then the moves:** it moves `TerminalUtil`, then `PatchWatcher`, behind the native library, one commit each.
  - **Gate:** a move that measurably regresses latency doesn't land.
  - `CrdtReplicaManager` stays in Kotlin.

The two workers touch different parts of the plugin, and I'll integrate them one at a time.

**`#jb263-feed-verify`:** I struck its queue row. The todo was already done in the previous response.

**Review gate `#jb262dynlauncherrecommen` is resolved by your autoupgrade decision:**
- The 2026.2 package keeps the accurate-warning policy (`declined_by=modular_package`, then file replacement and the restart prompt).
- A real restart-free path for 2026.2 and 2026.3 is now tracked by `#jb339shelldesign` instead of a separate 2026.2-only launcher.

### Re: do [#jb339shelldesign] — opus-5 · 2026-10-26T20:54-04:00

> **Queue prompt:** do [#jb339shelldesign]

**On `main` as `d8f0db1ea6`. This is a design and a test-only spike; nothing users install changes.** The plugin version and source digest are unchanged, so no install was needed. Integrated check: `make check-fast` passed, including 971 JetBrains JUnit tests with 0 failures and the spike's 12 tests.

**The design (`editors/jetbrains/SPEC.md`, "Restart-free implementation generations"):**

- **The plugin never gets unloaded.** The installed plugin becomes a stable shell with the same id and package, and no extra plugin is installed. It loads the real code from a versioned `impl/` jar into a classloader agent-doc owns. An upgrade swaps that code in place and never calls `DynamicPlugins.unloadPlugin`, so the 2026.3 module-graph refusal no longer applies.
- **The IDE only ever holds shell objects.** A table maps every registration the plugin makes today (27 actions, services, message-bus topics, extensions, the Remote Dev RPC, threads, JNA, `Key`s) to the shell forwarder that owns it during a swap. The code being swapped is barred from registering anything directly.
- **Remote Dev:** each JVM publishes an activation receipt. Work that depends on the frontend runs only when the backend and frontend receipts match.
- **A restart is still needed for:** a shell change, an implementation jar built for a different shell version, a detected leak, the first install of the shell itself, and any update IntelliJ performs on its own (Marketplace, custom repository). Only agent-doc's own installer can swap without a restart. The restart prompt stays as the fallback.
- **Expected benefit:** a rough estimate from changed file paths says 50 of the last 82 plugin releases would have upgraded without a restart.
- **Staged plan:**
  - **R1:** shell and forwarders, code still in one loader. Costs one restart.
  - **R2:** load the code through the child loader at IDE start.
  - **R3:** live swaps on the classic plugin.
  - **R4:** live swaps on the 2026.2/2026.3 backend.
  - **R5:** move the remaining subsystems over.

**What the spike proves:**
- A jar is verified by sha256 and loaded in its own classloader.
- A swap keeps exactly one platform subscriber.
- The retired classloader really is garbage-collected. As a negative control, a deliberately leaky implementation pins its loader, and further swaps are refused.
- Bad jars are refused, and a failed swap rolls back.
- Two deliberate breakages each turned the garbage-collection test red.

**What it doesn't prove:** anything inside a real IDE (EDT, `Disposer`, `MessageBus`, JNA, Remote Dev, Marketplace). That is what R1–R4 are for.

**Risk:** this rewrites how the plugin owns its IDE registrations, about 1.6 MB of Kotlin across 89 files. Leaks show up only through natural garbage collection, and IntelliJ can't attribute errors in the swapped code to Agent Doc.

**Your decisions:**
- **Keep the frontend shell-only.** In Remote Dev, frontend changes still restart, and no code reaches your local JVM except through a plugin install. Frontend swaps are parked in the icebox as `#jbfrontendswap`.
- **R1 waits for `#jbshrinkrestarts`.** Its benchmark and moves report first, then you decide; the todo is `#jb339r1decision`. The same todo covers restating the 2026.3 reopen rule as "module graph and registrations unchanged across the swap, and the retired code is collectable", because this design never replaces the module graph.

### Re: todo id collision — opus-5 · 2026-10-26T20:55-04:00

**Fixed a mis-named todo.** The R1 decision todo from the previous response landed as `[#jbshrinkrestarts]` instead of `[#jb339r1decision]`. agent-doc ignored the explicit `id=` and took the first tracked id mentioned in the text. That todo shared its id with the running backlog item `#jbshrinkrestarts`, so closing either one could have closed the other.

I removed the mis-named todo and re-added it as `#jb339r1decision`, with no other id in its text. The backlog item and its running worker are untouched.

**Filed as `#todoaddidhijack`:**
- An explicit `id=` must always win.
- A todo must never take an id that an open tracked item already owns.
- A regression test should pin both.

### Re: do [#jbshrinkrestarts], do [#todoaddidhijack] — opus-5 · 2026-10-26T21:39-04:00

> **Queue prompt:** do [#jbshrinkrestarts]

**On `main` and installed:** benchmark `d34a3b53ab`, moves `82b1284035` and `ba299eef24`, and plugin bump `b2b22a4685` (JetBrains classic 0.2.845, 2026.2 0.2.846, 2026.3 0.2.847).

**Integrated checks:**
- `make check-fast` passed: 11739 run, 11739 passed.
- Classic Gradle passed: 984 tests, 0 failures.
- `jetbrains-262-check` and `jetbrains-263-check` both passed.

**Restart needed:** IntelliJ 2026.3 and Rider 2026.2 need one to load the bump, as expected. This is the first upgrade that should show the new **Restart IDE** balloon.

**New benchmark: `make jetbrains-keystroke-bench`.** It runs headless on a 127 KB document and fails if any keystroke case gets more than max(5 µs, 15%) slower. No regression:

| Case | Before | After |
|---|---|---|
| typing report | 106 µs | 106 µs |
| replica flush, 1 key | 54.7 ms | 49.0 ms |
| replica flush, 16 keys | 55.2 ms | 54.3 ms |
| worker hop | 2.4 µs | 2.0 µs |

**What moved into the native library** (it hot-reloads, so changing these rules no longer needs an IDE restart):
- **`route_outcome`:** how the plugin reads a Run Agent Doc route's output and every notification text. 32-case shared fixture.
- **`exchange_prefix_repair`:** the `❯ ` prefix repair applied after a patch. 126-case shared fixture.
- Both are exact ports, and the Kotlin stays as a fallback.
- The native route parse takes 38 µs versus 25 µs in Kotlin, once per route. The prefix repair takes 1.2 ms versus 0.3 ms, once per patch that needs it.

**The big finding:** a replica flush costs about 50 ms whatever the burst size. About 48 ms of that is native `agent_doc_replica_diff`, and it grows faster than the document (about 4.9 ms on a 32 KB copy). Queued below as `#replicadiffcost`.

> **Queue prompt:** do [#todoaddidhijack]

**On `main` as `da5f6f9473`, and installed.** `make check-fast` passed: 11451 run, 11451 passed. You chose the full fix:

- An explicit `id=` always wins.
- A todo never takes an id that an open review, backlog or icebox item already owns. An explicit collision is refused with a message naming the owner.
- A cited `#id` becomes a `(source: …)` link, so `--done <item>` still checks the linked todo off.
- `review_add` now runs the same id guard as backlog and icebox adds.
- Cause: the deliberate `#todoidlayer` reuse rule overrode an explicit id whenever the text cited exactly one open item. Five mutation checks each turn their tests red.

**One thing to know:** a link created by a `(source: …)` marker counts as "done when that item is done". So a todo that only mentions a prerequisite ("once X reports") would be checked off when X finishes. The worker flagged it; it's not queued.

**Queued (4), your picks:**

- `#replicadiffcost`: make the replica diff proportional to the edit. Native-only, gated by the keystroke benchmark.
- `#jbsessioncmdoutcome`: move the clear/restart refusal parsers into native code, the same way `route_outcome` moved.
- `#jbshellr1`: GH #339 stage R1. That's the shell, the forwarders and the activation receipts, with the code still in one loader. It costs one restart. It also proposes the restated 2026.3 reopen rule for your agreement. Todo `#jb339r1decision` is done.
- `#editorbackendparity`: your question.
  - Does VS Code on Coder have a similar frontend/backend split (remote extension host vs UI extensions)?
  - What does Zed's architecture imply?
  - What does each need for parity with the JetBrains plugins (restart-free upgrades, activation receipts, measured-layout routing)? The parity matrix goes in `editors/SPEC.md`.

### Re: split the work in subagents — opus-5 · 2026-10-26T21:42-04:00

> **Chat prompt (#chatprompt:46b061b44732):** split the work in subagents

**Dispatched four items, one worker each.** All four are 🚧 running, each in its own worktree under `~/work/btakita/agent-doc-wt/`, based on `main` `da5f6f9473`:

| Item | Worker | Branch | Touches |
|---|---|---|---|
| `#replicadiffcost` | `subagent:replicadiff` | `perf/replica-diff-proportional` | native replica diff only (no restart) |
| `#jbsessioncmdoutcome` | `subagent:jbsessioncmd` | `fix/jb-session-command-outcome` | `TerminalUtil.kt` parsers → native code |
| `#jbshellr1` | `subagent:jbshellr1` | `feat/jb339-shell-r1` | plugin shell + forwarders + receipts |
| `#editorbackendparity` | `subagent:editorparity` | `docs/editor-backend-parity` | research + `editors/SPEC.md` matrix |

**Keeping the workers out of each other's way:** each worker knows what its siblings are doing. `jbsessioncmd` stays inside the `TerminalUtil` parsing, `jbshellr1` stays out of it, and `editorparity` writes only `editors/SPEC.md`.

**Integration:**
- One at a time, in the order they report.
- One plugin version bump covers both Kotlin branches.
- `replicadiffcost` has to beat the keystroke benchmark before it lands.

### Re: do [#editorbackendparity] — opus-5 · 2026-10-26T22:00-04:00

> **Queue prompt:** do [#editorbackendparity]

**On `main` as `9aca9eec10`.** It adds a parity matrix as `editors/SPEC.md` § 13, a pointer in `editors/PARITY.md`, and an `editor_contract` test. The test fails whenever code closes a gap the matrix still claims, or the matrix drifts from `plugin-parity.tsv`. `make check-fast` passed: 5830 run, 5830 passed. It's docs and a test only, so there's nothing to install.

**Does VS Code on Coder have a similar architecture? Only partly.**

- **What's the same:** the UI runs locally (Desktop through Remote-SSH) or in the browser (code-server or VS Code Web), and extensions run in an extension host on the workspace.
- **What's different:** a VS Code extension is never split into frontend and backend halves. It runs whole in one host, and VS Code's API carries editor, tab and terminal state across. So the JetBrains-style version mismatch between a frontend and a backend can't happen.
- **What VS Code still lacks:**
  - We don't declare `extensionKind`.
  - On a remote connection, an update still needs a window reload; "restart extensions", added in 1.88, is local only.
  - Nothing detects an installed but not-yet-running VSIX.
  - Split orientation is assumed rather than measured.
  - `plugin install vscode` doesn't reach code-server.

**What Zed's architecture implies.**

- **Extensions:** they're sandboxed WASM with a narrow surface and no access to panes or splits. Zed already reloads a changed extension without a restart.
- **Remote:** it pushes extensions to a headless server whose version must match exactly.
- **So:** our WASM extension already acts as the stable shell. The work is in the `zed-lsp` process it launches.
  - It needs a way to pick up a new binary without restarting Zed.
  - Its registration carries no editor kind or version.
  - It can't do measured-layout routing, because Zed exposes no layout, so routing is by document only.

**Matrix** (upgrade · version proof · layout routing · status):

- **JetBrains classic:** dynamic reload, native code hot-reloads · single process · measured · supported
- **JetBrains 262/263:** restart only · receipts designed, not shipped · measured by the client · supported
- **VS Code local:** restart extensions · version sent at registration only · tab-group order · supported
- **VS Code on Coder:** window reload · same as local · same as local · runbook only
- **Zed local / remote:** extension reload, binary waits for a language-server restart · `serverInfo` only · none · staged

**Queued as the 3 grouped items you chose:**
- `#vscoderemoteparity`: `extensionKind`, activation receipt, install path, Coder smoke test.
- `#vscodeupgradelayout`: stale-VSIX detection and a reload prompt, measured orientation, layout rules behind native derive.
- `#zedparity`: registration identity, binary pickup, document-only routing, remote qualification.

### Re: parity follow-ups dispatched — opus-5 · 2026-10-26T22:02-04:00

**Dispatched all three parity items.** Each runs in its own worker and worktree, based on `main` `9aca9eec10`, and all three are 🚧 running:

| Item | Worker | Branch | Owns |
|---|---|---|---|
| `#vscoderemoteparity` | `subagent:vscoderemote` | `feat/vscode-remote-parity` | `extensionKind`, the activation receipt, the install path, a Coder smoke test |
| `#vscodeupgradelayout` | `subagent:vscodeupgrade` | `feat/vscode-upgrade-layout` | stale-VSIX detection and reload prompt, `getEditorLayout`, layout rules moved behind native derive |
| `#zedparity` | `subagent:zedparity` | `feat/zed-parity` | `zed-lsp` identity, binary pickup, document-only routing, remote qualification |

There is no Coder workspace or Zed remote host here, so each remote check ships as a script plus a runbook entry. The workers will say plainly what they actually exercised.

The two VS Code workers have split the extension's files between them, and I'll integrate them one at a time.

**Still running from earlier:** `#replicadiffcost`, `#jbsessioncmdoutcome` and `#jbshellr1`.

### Re: integration of the parallel batch — opus-5 · 2026-10-26T22:55-04:00

**Incident first.** The previous session ended because the `vscoderemote` worker killed your real tmux server.
- **What happened:** it cleaned up a local code-server test with `TMUX_TMPDIR=<scratch> tmux kill-server`, but `$TMUX` was still set from this pane. `$TMUX` overrides `TMUX_TMPDIR`, so tmux sessions 0, 1 and 2 all died.
- **What you need to do:** restart any agent sessions that were running in those panes.
- **What I changed:**
  - I recorded the rule in operator memory: never run `kill-server` / `kill-session` without `-L <socket>` and `env -u TMUX`.
  - I sent the rule to that worker.
  - I'll put it in every brief that touches tmux.

> **Queue prompt:** do [#jbsessioncmdoutcome]

**Merged and installed** (`3e84bf5a49`, bump `8e9ace65b3`).
- **What moved:** Clear/Restart output parsing and its messages are now the native `session_command_outcome` derivation.
- **Tests:**
  - A 51-case shared fixture checks the Rust code against the Kotlin fallback.
  - No keystroke regression.
  - Integrated checks: check-fast 5630 run, 5630 passed. Classic 990 JUnit, 0 failures. 2026.2 and 2026.3 checks pass.

> **Queue prompt:** do [#replicadiffcost]

**Merged and installed** (`dc8a670516`).
- **Result:** the native replica flush now costs the same at every document size.

  | Document | Flush (1 key) before | after |
  |---|---|---|
  | 125 KB | 21.7 ms | 59 µs |
  | 500 KB | 305 ms | 203 µs |

- **The three causes:**
  - Each local edit re-ordered the whole document.
  - `diff` scanned every element.
  - The state vector was recomputed on every flush.
- **The fix:** a chunked visible-order index, a journal of local edits that answers `diff`, and an in-place version vector.
- **Safety:** it falls back to lazily's own path whenever its view disagrees with the CRDT, and the displayed text still always comes from lazily.
- **Tests:** differential tests run against lazily's paths, and five mutation checks turn them red. check-fast 6278 run, 6278 passed. No IDE restart is needed.

> **Queue prompt:** do [#jbshellr1]

**First part merged and installed** (`42b99769da`, bump `1e918d5eea`).
- **What landed:**
  - All 27 actions are now shell forwarders.
  - Services, settings and JNA are classified as shell.
  - The shell and implementation digests are split.
  - `agent_doc_activation_receipt/v1` is logged on both JVMs and sent in a schema-5 snapshot.
- **Tests:** 1002 classic JUnit tests, 0 failures. 2026.2, 2026.3 and `verifyClassicArtifact` pass.
- **The rest of R1** is queued as one sequential item, as you chose: `#jbshellr1rest`.

**Reopen rule adopted** (`23411fcd7e`), as you chose.
- **New point (1):** the 2026.3 rule now requires the module graph, descriptor and every registration to stay unchanged across a swap, and the retired classloader to be collected.
- **Unchanged:** point (2), the activation receipts.
- `ModularRestartBoundaryContractTest` now pins the adopted wording.

> **Queue prompt:** do [#vscodeupgradelayout]

**Merged and installed** (`46fea05daf`, VS Code 0.2.84 `019432c698`).
- **Stale-version prompt:** the extension compares its running version with its own `extensions.json`. It offers Restart Extensions locally and Reload Window on remote.
- **Measured layout:** every layout capture now uses `vscode.getEditorLayout`, so a vertical split becomes one stacked column. The grouping rules are the native `editor_group_layout` derivation, checked against an 18-case fixture.
- **Merge note:** I resolved one conflict at integration (both derivations kept). `native.ts` `EDITOR_PLUGIN_VERSION` moved to 0.2.84 too, because the manifest guard pins it.
- **Tests:** check-fast 6259 run, 6259 passed. VS Code `npm test` 216 of 216 passed.
- **Not tried in a live VS Code window yet.**

> **Queue prompt:** do [#zedparity]

**Merged** (`379e4d97d6`, Zed extension 0.1.2 `e8a014620e`).
- **Registration identity:** `zed-lsp` now sends editor kind, version, binary version, capabilities, and `layout_authority=document_only`.
- **Binary pickup:** an in-place `execve` handoff keeps the same PID and stdio. The new binary must first validate the handoff; on refusal the old one keeps serving and shows a restart prompt. An end-to-end test swaps the binary mid-request.
- **Document-only routing:** enforced in the controller.
- **Remote qualification:** not done. `editors/zed/REMOTE.md` and `scripts/zed_remote_qualify.py` exist, but no Zed remote host was available.
- **Tests:** check-fast 11775 run, 11775 passed.

**Still running:** `#vscoderemoteparity`.
- **Done in its branch:** `extensionKind`, the activation receipt, and an install path that reaches code-server and Remote-SSH.
- **In progress:** merging main into its branch (no force-push).

> **Queue prompt:** [#ahfe]: I believe it works now. verify

**Partly verified. The todo stays open.**
- **What the logs show:**
  - IntelliJ 2026.3 hot-reloaded `libagent_doc` without a restart at 21:32, 22:13 and 22:53.
  - Run Agent Doc routes after the 22:15 IDE start finished with exit 0 (02:27Z and 02:36Z).
  - The IDE now runs plugin 0.2.853, so you did restart.
- **What GH #267 needs and I can't confirm:** a route run while an *older* plugin is loaded against a newer binary. That should log `editor_route_superseded_editor_admitted` and end the route output with the advisory line. That case hasn't happened since the fix, and I can't press Run Agent Doc.
- **The next JetBrains bump** (`#jbshellr1rest`) creates that case. After its install, don't restart. Wait for `[native] hot-reloaded libagent_doc` in `idea.log`, run Run Agent Doc once, and I'll check `ops.log`.

> **Queue prompt:** Watch https://github.com/btakita/agent-doc/issues for new issues. Add new issues into the backlog + queue except issue #40 + #45.

No new issues this pass. The open ones are #379 (tracked by `#gh379-remotedev-proof`), #45 and #40.

**Queued:**
- `#jbshellr1rest`: the rest of R1, in one worker.
- `#vscodesessionparsers`, `#replicaremoteflush`, `#kotlinflushprep`: your picks.
- Two defects `vscoderemote` found on unmodified main:
  - `#vscodekoffivsix`: the shipped VSIX has no koffi, so a VSIX install runs without the native library and never registers.
  - `#vscodekoffisegv`: the extension host crashes with SIGSEGV in koffi's `free` under code-server.
- **Dispatch order:** `#jbshellr1rest` and `#replicaremoteflush` now. The three VS Code items wait until `vscoderemote` lands, and `#kotlinflushprep` waits until `#jbshellr1rest` lands, so no two workers edit the same plugin files at once.

**Needs you:** restart IntelliJ 2026.3 when convenient. VS Code now has 0.2.84 installed; reload the window to load it.

### Re: VS Code remote parity, sdk-demo turn — opus-5 · 2026-10-26T23:05-04:00

> **Queue prompt:** do [#vscoderemoteparity]

**On `main` and installed.** The worker merged main into its branch twice, as merge commits (no force-push), giving `10e342c30e`. VS Code is now 0.2.85 (`6417f3d014`). I bumped past 0.2.84 because a 0.2.84 VSIX without these changes was already installed here.

- **`"extensionKind": ["workspace"]`:** confirmed loading in code-server 4.143.1's server-side extension host.
- **Activation receipt:** every registration carries one `agent_doc_activation_receipt/v1 role=workspace` line. The controller parses it from an optional `activation_receipt` field, so older controllers are unaffected, and `agent-doc reliable-sync-status` prints it.
- **Install path:** `plugin install|update|list vscode` now also reaches `code-server` and the newest Remote-SSH server (`~/.vscode-server`, `~/.vscodium-server`, `~/.cursor-server`). This worked against a real code-server and a real VS Code Server.
- **Coder smoke test:** `scripts/vscode-remote-smoke.py` is written, and its procedure is in `runbooks/coder-workspace.md`. Locally, install, register and remote host passed. It has not been run on Coder, so I added todo `#vscodecodersmoke` for you.
- **Integrated checks:** check-fast 5857 run, 5857 passed. VS Code `npm test` 220 of 220 passed.

**Dispatched:** the three held VS Code items are 🚧 running in their own worktrees.
- `#vscodesessionparsers`: move `sessionUi.ts` onto the native `session_command_outcome`.
- `#vscodekoffivsix`: ship koffi in the VSIX.
- `#vscodekoffisegv`: find the cause of the koffi `free` SIGSEGV.

`#jbshellr1rest` and `#replicaremoteflush` are still running. `#kotlinflushprep` waits for `#jbshellr1rest`. Every brief now carries the tmux safety rule.

> **Queue prompt:** sdk-demo.md turn did not complete. Response is uncommitted.

**It isn't stuck. The turn is still running, so I left it alone.** Checked read-only at 03:03Z:
- **The cycle:** `src/haiven-dev/tasks/sdk/sdk-demo.md` has cycle `cycle-1793070156281`, which started at 03:02Z and is in `preflight_started`. Its owning pane `%3` holds a fresh active-turn lease.
- **The pane:** its main agent is waiting at the prompt. One of its 3 background workers, "Fix Boss Room session.end telemetry", is still running.
- **Why it's uncommitted:** the response commits when that worker reports and the owner closes the turn. The uncommitted change you see in `sdk-demo.md` is that turn's work in progress.
- **The previous turn:** it committed normally as `d29f4a52` at 22:31.
- `session-check` says explicitly not to recover, re-finalize or clear it from another pane, so I didn't.
- **Why I didn't start a worker:** the claim I took for this row was released (requeued) without one, since a worker would only have waited on the same turn.

If it's still uncommitted after that worker finishes, run `agent-doc session-check src/haiven-dev/tasks/sdk/sdk-demo.md` and queue it again. A finished worker with no commit would be a real defect.

### Re: VS Code native-library fixes — opus-5 · 2026-10-26T23:13-04:00

> **Queue prompt:** do [#vscodekoffisegv]

**Found and fixed.** On `main` as `6431911c50` (VS Code 0.2.86, `636b72eb52`), installed.

**Cause:**
- `editors/vscode/src/native.ts` declared Rust-owned result strings (struct `text`/`json`/`error` fields, plus two return values) as `char*`.
- koffi turns a `char*` into a JS string copy, so the Rust pointer was lost.
- The wrapper's `finally` block then passed that JS string to `agent_doc_free_string(void*)`. koffi turned it into a pointer to its own temporary copy, and Rust called libc `free` on memory it never allocated.
- **Effect:** every *successful* reposition, normalize or apply-node-patches call crashed the extension host, and every error-path call leaked.
- The Rust side and the JetBrains JNA binding were already correct.

**Fix:**
- Every owned field and return value is now an opaque pointer.
- All 22 free call sites go through one `freeOwnedCString` that refuses anything but a real pointer.
- It also handles koffi 3's BigInt pointers.

**Proof:**
- With the unmodified wrappers, a node script crashes with exit 139. The gdb backtrace matches the report: `free` ← `CString` drop ← `agent_doc_free_string`.
- A new child-process test drives the real wrappers against the built library and fails with "child died by SIGSEGV" against the old declarations.
- Integrated: check-fast passed, VS Code 224 of 224.

> **Queue prompt:** do [#vscodekoffivsix]

**Confirmed and fixed.** On `main` as `91719d8a58` (VS Code 0.2.87, `07b9591e97`), installed.

**Confirmed:**
- Every VSIX so far shipped without koffi: the 0.2.85 package had no `node_modules`, and neither did any installed copy from 0.2.81 to 0.2.85.
- So a VSIX install never loaded the native library and never registered with the controller, on Coder or anywhere else.
- koffi 3.3.3 also splits its binaries into per-platform packages, so allow-listing `node_modules` alone would have shipped Linux x64 only.

**Fix:**
- A new `scripts/stage-koffi.mjs` runs in `vscode:prepublish`. It stages koffi binaries for linux-x64/arm64 (glibc and musl), darwin-x64/arm64 and win32-x64. Each download is checked against `package-lock.json`.
- `.vscodeignore` now lets through only koffi's runtime and those binaries.
- It's still one VSIX for every platform. It grew from 129 KB to 2.9 MB.

**Packaging test** (`npm run test:package`):
- It unzips the VSIX outside the checkout and checks every platform binary's header.
- It refuses any other `node_modules` content.
- It makes a real `getpid()` call through the packaged koffi.
- It runs in `make check`, in `make install-editor-plugins` (which refuses to install a VSIX that fails it), and in the release workflow before publishing.
- It fails 4 of 4 on the old 0.2.85 VSIX and passes 4 of 4 now.

**Checks:**
- Integrated: check-fast passed, VS Code 224 of 224, packaging 4 of 4.
- The installed `btakita.agent-doc-0.2.87` now contains `node_modules/koffi/build/koffi/` for all 7 platforms.

**Before these two fixes,** the VS Code extension had effectively been running without its native library. Reload VS Code to load 0.2.87; it's the first build where registration, layout and receipts can actually work end to end.

**Dispatched:** the JetBrains `EditorFocusSyncListener` startup crash you reported on `sdk-demo.md` (`lateinit property mainSplitters`) → `subagent:focussyncinit` (🚧). `projectOpened` calls `getSplitters()` before the 2026.3 editor area exists. The worker will defer attaching the listener until the editor area exists, and will check that no later startup step can be skipped by one listener's failure.

**Still running:** `#jbshellr1rest`, `#replicaremoteflush`, `#vscodesessionparsers`. `#kotlinflushprep` waits for `#jbshellr1rest`.

### Re: do [#vscodesessionparsers] — opus-5 · 2026-10-26T23:19-04:00

> **Queue prompt:** do [#vscodesessionparsers]

**On `main` and installed** (`064d649ded`, VS Code 0.2.88 `b2700613ad`).

**What moved:**
- VS Code's Clear and Restart Supervisor output now goes through the native `session_command_outcome` derivation, the same one JetBrains uses.
- The new `sessionCommandOutcome.ts` asks `agent_doc_editor_derive_json`. It falls back to a TypeScript copy that answers all 51 cases of the one shared fixture exactly like native, so no divergences were needed.
- The old regexes in `sessionUi.ts` are gone.

**What changes for you in VS Code:**
- Clears that were queued or deferred now say so. Before, VS Code read only stdout and reported "Cleared session context" even when nothing had cleared.
- A clear refusal is recognised for `active_agent_doc` and `busy` panes too, not only `alive-busy`.
- A restart refused because a terminal editor holds the pane is now a warning with "Show status" and "Copy details". Before, it was a generic error, and an unhandled rejection under "Interrupt and restart".

**Checks:**
- Integrated: check-fast 5658 run, 5658 passed.
- VS Code 225 of 225.
- Four deliberate breakages of the fallback each turned the new tests red.

### Re: JetBrains startup crash on sdk-demo.md — opus-5 · 2026-10-26T23:23-04:00

> **Queue prompt:** Run Agent Doc on sdk-demo.md error
> ```
> kotlin.UninitializedPropertyAccessException: lateinit property mainSplitters has not been initialized
> 	at com.intellij.openapi.fileEditor.impl.FileEditorManagerImpl.getMainSplitters(FileEditorManagerImpl.kt:231)
> 	at com.intellij.openapi.fileEditor.impl.FileEditorManagerImpl.dockable$lambda$0(FileEditorManagerImpl.kt:239)
> 	at kotlin.SynchronizedLazyImpl.getValue(LazyJVM.kt:86)
> 	at com.intellij.openapi.fileEditor.impl.FileEditorManagerImpl.getDockContainer(FileEditorManagerImpl.kt:313)
> 	at com.intellij.ui.docking.impl.DockManagerImpl.getFileManagerContainer(DockManagerImpl.kt:558)
> 	at com.intellij.ui.docking.impl.DockManagerImpl.getAllContainers(DockManagerImpl.kt:554)
> 	at com.intellij.ui.docking.impl.DockManagerImpl.getContainerFor(DockManagerImpl.kt:175)
> 	at com.intellij.openapi.fileEditor.impl.FileEditorManagerImpl.getDockContainer(FileEditorManagerImpl.kt:697)
> 	at com.intellij.openapi.fileEditor.impl.FileEditorManagerImpl.getActiveSplitterSync(FileEditorManagerImpl.kt:690)
> 	at com.intellij.openapi.fileEditor.impl.FileEditorManagerImpl.getSplitters(FileEditorManagerImpl.kt:1939)
> 	at com.github.btakita.agentdoc.EditorFocusSyncListener.attachEditorTreeMouseListener$lambda$0(EditorFocusSyncListener.kt:183)
> 	at com.github.btakita.agentdoc.EditorFocusSyncListener.attachEditorTreeMouseListener(EditorFocusSyncListener.kt:205)
> 	at com.github.btakita.agentdoc.EditorFocusSyncListener.<init>(EditorFocusSyncListener.kt:177)
> 	at com.github.btakita.agentdoc.EditorFocusSyncListener.<init>(EditorFocusSyncListener.kt)
> 	at com.github.btakita.agentdoc.EditorFocusSyncListener$Companion.install$lambda$0(EditorFocusSyncListener.kt:250)
> 	at com.github.btakita.agentdoc.EditorFocusSyncListener$Companion.install$lambda$1(EditorFocusSyncListener.kt:250)
> 	at java.base/java.util.concurrent.ConcurrentHashMap.computeIfAbsent(ConcurrentHashMap.java:1724)
> 	at com.github.btakita.agentdoc.EditorFocusSyncListener$Companion.install(EditorFocusSyncListener.kt:250)
> 	at com.github.btakita.agentdoc.PluginLifecycleListener.projectOpened(PluginLifecycleListener262Generated.kt:87)
> 	at com.intellij.util.messages.impl.MessageBusImplKt.invokeMethod(MessageBusImpl.kt:848)
> 	at com.intellij.util.messages.impl.MessageBusImplKt.invokeListener(MessageBusImpl.kt:789)
> 	at com.intellij.util.messages.impl.MessageBusImplKt.executeOrAddToQueue(MessageBusImpl.kt:610)
> 	at com.intellij.util.messages.impl.ToDirectChildrenMessagePublisher.publish$intellij_platform_core(CompositeMessageBus.kt:193)
> 	at com.intellij.util.messages.impl.MessagePublisher.invoke(MessageBusImpl.kt:558)
> 	at jdk.proxy2/jdk.proxy2.$Proxy91.projectOpened(Unknown Source)
> 	at com.intellij.openapi.project.impl.ProjectManagerImplKt$runInitProjectActivities$2.invokeSuspend(ProjectManagerImpl.kt:1636)
> 	at kotlin.coroutines.jvm.internal.BaseContinuationImpl.resumeWith(ContinuationImpl.kt:34)
> 	at kotlinx.coroutines.DispatchedTask.run(DispatchedTask.kt:100)
> 	at com.intellij.openapi.application.impl.DispatchedRunnable.run(DispatchedRunnable.kt:42)
> 	at com.intellij.openapi.application.TransactionGuardImpl.runWithWritingAllowed(TransactionGuardImpl.java:246)
> 	at com.intellij.openapi.application.TransactionGuardImpl.access$100(TransactionGuardImpl.java:22)
> 	at com.intellij.openapi.application.TransactionGuardImpl$2.run(TransactionGuardImpl.java:228)
> 	at com.intellij.openapi.application.impl.NonBlockingFlushQueue.runNextEvent$lambda$1$1$0(NonBlockingFlushQueue.kt:329)
> 	at com.intellij.openapi.progress.impl.CoreProgressManager.computePrioritized(CoreProgressManager.java:963)
> 	at com.intellij.openapi.application.impl.NonBlockingFlushQueue.runNextEvent$lambda$1$1(NonBlockingFlushQueue.kt:328)
> 	at com.intellij.concurrency.ThreadContext.resetThreadContext(threadContext.kt:295)
> 	at com.intellij.openapi.application.impl.NonBlockingFlushQueue.runNextEvent$lambda$1(NonBlockingFlushQueue.kt:325)
> 	at com.intellij.platform.locking.impl.NestedLocksThreadingSupport.tryRunWriteIntentReadAction(NestedLocksThreadingSupport.kt:786)
> 	at com.intellij.openapi.application.impl.NonBlockingFlushQueue.runNextEvent(NonBlockingFlushQueue.kt:321)
> 	at com.intellij.openapi.application.impl.NonBlockingFlushQueue.flushNow(NonBlockingFlushQueue.kt:299)
> 	at com.intellij.openapi.application.impl.NonBlockingFlushQueue.FLUSH_NOW$lambda$0(NonBlockingFlushQueue.kt:158)
> 	at java.desktop/java.awt.event.InvocationEvent.dispatch(InvocationEvent.java:323)
> 	at java.desktop/java.awt.EventQueue.dispatchEventImpl(EventQueue.java:732)
> 	at java.desktop/java.awt.EventQueue.dispatchEvent(EventQueue.java:711)
> 	at com.intellij.ide.IdeEventQueue.defaultDispatchEvent(IdeEventQueue.kt:749)
> 	at com.intellij.ide.IdeEventQueue._dispatchEvent(IdeEventQueue.kt:602)
> 	at com.intellij.ide.IdeEventQueue.access$_dispatchEvent(IdeEventQueue.kt:143)
> 	at com.intellij.ide.IdeEventQueue$dispatchEvent$processEventRunnable$1$1$1$1.invoke(IdeEventQueue.kt:401)
> 	at com.intellij.ide.IdeEventQueue$dispatchEvent$processEventRunnable$1$1$1$1.invoke(IdeEventQueue.kt:399)
> 	at com.intellij.ide.IdeEventQueueKt.performActivity$lambda$0(IdeEventQueue.kt:1136)
> 	at com.intellij.openapi.application.TransactionGuardImpl.performActivity(TransactionGuardImpl.java:106)
> 	at com.intellij.ide.IdeEventQueueKt.performActivity(IdeEventQueue.kt:1136)
> 	at com.intellij.ide.IdeEventQueue.dispatchEvent$lambda$0(IdeEventQueue.kt:399)
> 	at com.intellij.ide.IdeEventQueue.dispatchEvent(IdeEventQueue.kt:440)
> 	at java.desktop/java.awt.EventDispatchThread.pumpOneEventForFilters(EventDispatchThread.java:207)
> 	at java.desktop/java.awt.EventDispatchThread.pumpEventsForFilter(EventDispatchThread.java:128)
> 	at java.desktop/java.awt.EventDispatchThread.pumpEventsForHierarchy(EventDispatchThread.java:117)
> 	at java.desktop/java.awt.EventDispatchThread.pumpEvents(EventDispatchThread.java:113)
> 	at java.desktop/java.awt.EventDispatchThread.pumpEvents(EventDispatchThread.java:105)
> 	at java.desktop/java.awt.EventDispatchThread.run(EventDispatchThread.java:92)
> ```

**Fixed. On `main` as `76208ef0aa` (JetBrains classic 0.2.854, 2026.2 0.2.855, 2026.3 0.2.856, `2e3a03ca2e`), installed.** IntelliJ 2026.3 needs a restart to load it.

**Cause:**
- `PluginLifecycleListener.projectOpened` calls `EditorFocusSyncListener.install`.
- Its constructor called `FileEditorManagerEx.splitters` synchronously.
- On 2026.3, `projectOpened` can arrive before the platform has built the editor area, so `getSplitters()` threw `UninitializedPropertyAccessException`.
- The exception escaped `install`, so every later startup step in `projectOpened` was skipped: IDE activation, `TmuxPaneFocusSync`, the `FileEditorManager` subscriptions, open-document registration and the rename listener. That is why Run Agent Doc broke afterwards.

**Fix** (all in `EditorFocusSyncListener.kt`):
- **Narrow catch:** only that one not-built-yet exception is treated as "not ready". Any other failure still throws.
- **Retry:** the editor-tree mouse listener retries on the first `fileOpened` / `selectionChanged`. No tab can exist before the split tree does. After attaching it unsubscribes.
- **Contained failure:** install now runs its work isolated, so a failed start is logged and disposed and can never abort `projectOpened` again. Cancellation still propagates.
- **API choice:** `StartupManager.runAfterOpened` is internal API in 2026.3, so the fix doesn't use it.

**Tests:**
- 6 new tests, including one that fakes the exact not-ready exception and shows a later attach happens exactly once.
- Changing the caught exception type turns 2 tests red.
- Integrated: check-fast passed. Classic 1008 JUnit tests, 0 failures. `jetbrains-262-check` and `jetbrains-263-check` pass.

**Also worth knowing:** this bump creates the `[#ahfe]` / GH #267 case. Before you restart IntelliJ, if you want to finish that check, wait for `[native] hot-reloaded libagent_doc` in `idea.log`, then run Run Agent Doc once. The old 0.2.853 plugin routing against the new binary should log `editor_route_superseded_editor_admitted`. Then restart to load 0.2.856.

**Still running:** `#jbshellr1rest` and `#replicaremoteflush`. `#kotlinflushprep` waits for `#jbshellr1rest`.

### Re: do [#jbshellr1rest] — opus-5 · 2026-10-27T00:03-04:00

> **Queue prompt:** do [#jbshellr1rest]

**GH #339 stage R1 is complete.** On `main` and installed: steps `7777ce8e60` → `df51b62d94` (rebased), bump `23ba8936a9`, generator fix `b9ceed571d`, re-bump `b62225007a`. JetBrains classic is 0.2.857, 2026.2 0.2.860, 2026.3 0.2.861. IntelliJ 2026.3 needs a restart to load it.

**The five steps, one commit each:**
1. Project lifecycle and all 17 programmatic listener registrations go through shell forwarders. The lifecycle body moved unchanged into `AgentDocProjectLifecycle`.
2. The six editor and language extensions (completion, folding, gutter, structure view, status-bar widget, banner) are shell forwarders that build the platform objects from plain data.
3. EDT work goes through `GenerationContext.invokeLater` → `FencedTask`. Each generation has its own executors, threads, alarm and `CoroutineScope`. On retirement, `CANCEL` is for the R3 swap and `DRAIN` is for plugin unload.
4. The JetBrains `Register` op now sends the backend activation receipt, in the `activation_receipt` field VS Code already added. A shared fixture is checked from both Kotlin and Rust.
5. User-data keys are created by the shell. The 2026.2/2026.3 Fleet RPC ingress only delegates, through `SurfaceIngressSpi`, and its state moved into the implementation.

**Audit:**
- `ShellRegistrationOwnershipTest` now enforces **zero** direct bus subscriptions, listeners, `invokeLater`, pooled or raw threads, executors, alarms or `Key.create` in implementation files.
- Still to move, recorded as R2+ in the spec: AWT listeners (3), notifications (21) and `putUserData` values (4).
- **Behaviour change:** one edge case. During plugin unload, a retired generation refuses new listener registrations and logs it.

**A build defect I fixed while integrating:**
- **Symptom:** in an existing build directory, `jetbrains-262-check` / `-263-check` failed with `Redeclaration`.
- **Cause:** `generateModularLifecycle` declared only its one output file. The old `PluginLifecycleListener262Generated.kt` survived next to the new `AgentDocProjectLifecycle262Generated.kt`.
- **Fix:** the task now owns and clears its output directory (`b9ceed571d`). Without it, every developer's incremental build and `make install` would have broken.

**Integrated checks:**
- check-fast (`env -u TMUX`): 4524 run, 4524 passed.
- Classic: 1027 JUnit tests, 0 failures. `verifyClassicArtifact` passes.
- `jetbrains-262-check` and `-263-check` pass, both on a dirty build directory and after the fix.

**Dispatched:** `#kotlinflushprep` → `subagent:kotlinflush` (🚧), now that R1 has landed.

**Also on main, from another session:** a PreToolUse guard that refuses agent commands that would kill your live tmux server (`edcfce58d0`, `bc3da0e033`, `e3b716ae8a`). It covers this morning's incident.

### Re: do [#replicaremoteflush] — opus-5 · 2026-10-27T00:08-04:00

> **Queue prompt:** do [#replicaremoteflush]

**On `main` as `60c7f4c73b`, installed.** Native only, so no IDE restart; the library hot-reloads.

**What changed:**
- **Remote ops fold into the index:** the replica's order index now covers the whole CRDT graph, deleted characters included. Remote ops are folded in as they arrive, using lazily's own placement rule, instead of the index being thrown away and rebuilt.
- **Out-of-order ops wait:** an op whose origin hasn't arrived is held and placed once the origin lands.
- **Big batches still rebuild:** more than 4,096 ops, or more than 1/8 of the graph.
- **Incremental bookkeeping:** the version vector updates in place. The journal now records remote changes too, so more peers get answered without a document scan.
- **Guard kept:** a folded order is used only after it spells exactly lazily's text. Otherwise that replica keeps lazily's paths for good, and displayed text always comes from lazily.

**Numbers** (125 KB, `p50_best`; the gated benchmark reports "no keystroke regression"):

| Case | Before | After |
|---|---|---|
| first local flush after a remote update (new case) | 46–74 ms | 166–172 µs |
| `replica_flush_1` | 58–64 µs | 59–60 µs |
| `typing_report` | 108–116 µs | 108 µs |

Applying the remote update itself is unchanged (≈64–71 ms), because lazily's own text projection dominates it and the editor needs that text anyway.

**Tests:**
- **Differential runs:** 1–3 concurrent peers, covering shared-offset bursts, out-of-order and duplicate delivery, multibyte text, and local deletes of remote text. After every step they compare the full order (deleted characters and held ops included) and the version vector against lazily.
- **Property test:** `folded_order_equals_lazily_after_every_remote_batch`.
- **Mutation checks:** 11 of 11 mutations are caught.
- **Integrated:** check-fast 6327 run, 6327 passed.

**Possible follow-up, not queued:** after a remote update, the first local splice walks back from the end of the document to find its position, about 57 µs at 125 KB. This is older behaviour, not part of this change.

**Still running:** `#kotlinflushprep`.

### Re: do [#kotlinflushprep] — opus-5 · 2026-10-27T00:24-04:00

> **Queue prompt:** do [#kotlinflushprep]

**Partly done: faster, but not fully proportional to the edit yet.** On `main` as `0e06cf94d4` (JetBrains classic 0.2.862, 2026.2 0.2.863, 2026.3 0.2.864, `d1155e8943`), installed. IntelliJ needs a restart to load it.

**Cause:** the old `prepareLocalEditorEditsUtil` (`CrdtReplicaManager.kt:1082-1124`) did three things that grew with the document:
- copied the whole shadow into a `StringBuilder`;
- counted code points over the whole shadow, and these documents contain multibyte characters such as `—`, `❯` and `✓`;
- copied the whole document again for `resultingText`.

**Fix** (new `LocalEditorSplicePreparation.kt`, same function name and signature):
- **Edit window:** a burst is applied to one window over the shadow, and only that window is read.
- **Offset conversion:** UTF-16 offsets become code points through an index of the shadow's surrogate pairs. The index is cached by String identity and handed to the next flush, so typing never rescans the document.
- **Same output:** the native edits are the same as before.

**What's left:** each flush still builds the next shadow as a whole `String`. That copy alone is ≈35 µs at 125 KB and ≈170 µs at 500 KB, which is all `prepare` still costs. Removing it needs a rope or piece-table shadow in `CrdtReplicaManager` and `CrdtReplicaForwarder`. That's a bigger, riskier change for tens of µs per flush, so I parked it in the icebox as `#jbropeshadow` instead of queueing it. Move it to the backlog if you want it.

**Numbers** (`p50_best`, before → after; the gate passes at all sizes):

| Size | `prepare` (1 / 16 keys) | `replica_flush_16` | `typing_report` |
|---|---|---|---|
| 32 KB | 8.8 / 10.5 → 5.6 / 7.3 µs | 54.8 → 52.7 µs | 38.0 → 39.2 µs |
| 125 KB | 33.8 / 36.3 → 25.7 / 23.1 µs | 94.7 → 71.7 µs | 116.8 → 108.1 µs |
| 500 KB | 229 / 269 → 177 / 179 µs | 332 → 229 µs | 494 → 400 µs |

**Tests:**
- A differential test runs the old implementation, copied verbatim, beside the new one on 4,000 randomized chained bursts. They cover multibyte text, surrogate pairs, unpaired halves and the three refusals.
- Six deliberate bugs in the window and index code were all caught.
- **Integrated:**
  - check-fast passed.
  - Classic plugin: 1032 JUnit tests, 0 failures.
  - `jetbrains-262-check` and `-263-check` pass.
  - I resolved one spec conflict, keeping both the remote-fold and flush-prep sections.

**Queue status:** the run queue has no open work left. Everything dispatched this session is merged and installed.

**Waiting on you:**
- Before restarting IntelliJ, run **Run Agent Doc** once to finish `[#ahfe]` / GH #267. Then restart to load 0.2.864.
- Reload VS Code to load 0.2.88.
- Run `#vscodecodersmoke` and `#gh379-remotedev-proof` on the remote hosts.

### Re: #gh-fix GH #380 + #381 — opus-5 · 2026-10-27T00:34-04:00

I dispatched both issues to background subagents. Each works in its own worktree outside agent-loop and pushes its own branch. When they report back, I'll integrate both into `main`, run one `make install`, push, and close both issues.

| Row | Worker | Worktree / branch | Issue |
|---|---|---|---|
| #380 | `subagent:gh380` gen 2 | `agent-doc-wt/gh380` · `fix/gh380-supervisor-spawn-path` | Supervisor can't spawn bare `claude` when the inherited PATH is the Coder default (no user-local bin) |
| #381 | `subagent:gh381` gen 1 | `agent-doc-wt/gh381` · `fix/gh381-project-preset-queue-head` | A queue head that is only a project preset (from `.agent-doc/project.md`) is undrainable and gets a missing-Todo warning |

**The #380 row's 🚧 marker had no worker behind it.** `queue claims` showed the row held by `subagent:kotlinflush` gen 1, carrying the `#kotlinflushprep` alias, with `followed_from: [do [#kotlinflushprep], Watch …]`. That worker was the `#kotlinflushprep` job, and it finished at 00:21. When that row was struck, its claim moved to the next head instead of ending. Earlier it had also moved onto the Loop watch row. So #380 had never been dispatched. I released the stray claim (`queue release --requeue --generation 1`), then claimed and started a real worker (gen 2). I filed the claim-move bug as backlog `#claimfollowstruck`.

The exchange is 778 lines over 22 sections. The queue is draining, so I'm not compacting.

### Re: Subagent completed: `subagent:gh380` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/380” — agent-doc
<!-- agent-doc-subagent-response sha256=1b0fbb2ddbb3d3aafc7611958b8979452d14d8123945563260f0eabfbfa3a008 -->
> **Subagent response:**
>
> Branch fix/gh380-supervisor-spawn-path, commit 82ff929303 (pushed). Files: agent-doc-supervisor-process/src/{spawn_program.rs (new), pty.rs, lib.rs}, agent-doc-start-runtime-io/src/run.rs, specs/supervisor.md.
> Root cause: PtySession::spawn clears env and portable-pty resolves the bare harness name only against the inherited controller PATH; the Coder default PATH omits ~/.local/bin. All four harnesses share this spawn.
> Fix: if the bare name is not on the child PATH, resolve it from well-known install dirs (~/.local/bin, ~/.claude/local, ~/.opencode/bin, ~/.grok/bin, npm/bun/volta/cargo bins, ~/bin, /usr/local/bin, Homebrew); spawn the absolute path (symlinks unresolved), append its dir to the child PATH, log harness_program_resolved source=install_dir_fallback; not-found error lists every dir searched.
> Tests: cargo test -p agent-doc-supervisor-process --lib -- spawn_program pty:: = 22 passed (8+2 new; regression bare_harness_name_missing_from_coder_default_path_spawns_from_user_local_bin fails with the fallback disabled). make check: clippy ok, Rust 11818/11818 passed (182 skipped); exits 2 on VS Code dev-harness-test "round-trips owned strings through the built library" (editors/vscode/src/native.test.ts:390, from 6431911c50), which the worker reports as pre-existing under make's project-root bound. One-off flake test_cli_claim_occupied_pane_persists_generated_identity_before_provisioning (controller generation CAS expected 1 found 2) passed 5/5 in isolation.
> Follow-ups: (1) VS Code native test resolveProjectPath under make's root bound; (2) claim-vs-supervisor generation CAS race in that CLI test; (3) optional: headless harness launches in agent-doc-agent-io (claude/codex/opencode/grok) use bare-name PATH lookup too.
<!-- agent-doc-subagent-completion key=357ed6d65b725f8e41502d578a4ac1f8543a91a031e49359893178d7cb484e2a -->

### Re: Subagent completed: `subagent:claimfollow` returned work for queue item “do [#claimfollowstruck]” — agent-doc
<!-- agent-doc-subagent-response sha256=b5eaff1d216857f77fb600147f6eb41e86e9c788d121273e99fb17160319e29f -->
> **Subagent response:**
>
> Branch fix/claimfollowstruck, commit 75ff627e58 (pushed).
> Root cause: every claim reader aligned anchors against the document's SELECTED queue (live_queue_head_rows -> queue_component_entries -> selected(), agent-doc-queue/src/queue_continuation.rs:1038/:1076), anchors carried no queue id, and a struck row was never treated as terminal. Striking the last agent:run row made agent:loop selected; the 1-row anchor paired with the loop row through the equal-count positional fallback in pair_replaced_lines (queue_head_alignment.rs ~150), then onto #380 when new run heads arrived. The relay overlay used the same selected-queue follow and painted markers on every queue.
> Fix: ClaimAnchor records queue_id; QueueClaimLedger::follow_edits_in_view over ClaimQueueView (all queues, live + struck rows) aligns each claim only within its own queue; a claim whose row is struck never follows a different head (existing #claimstrike retire / reopen-as-unknown applies). Self-repair: a same-attempt mis-follow (new QueueClaim.followed_generation) whose followed_from names a struck row is restored onto it (queue_claim_unfollowed_struck); re-claims and legacy claims are never restored. All readers migrated (queue-io claim paths, relay overlay, controller orphan drain, claimed_live_head_texts). In-place edits still follow.
> Files: agent-doc-queue/src/{queue_claim.rs, queue_consume.rs}, agent-doc-queue-io/src/queue_claim.rs, agent-doc-crdt-relay-io/src/lib.rs, agent-doc-controller-io/src/project_controller/rpc.rs, specs/07-orchestration-commands.md; field-only: agent-doc-queue/src/claim_release_receipt.rs, agent-doc-session-check-io/src/midturn_steering.rs, src/queue_completion_feedback.rs.
> Tests: 5 new agent-doc-queue tests + 1 queue-io ledger test; 4 deliberate mutations each reddened the matching test. cargo test -p agent-doc-queue --lib 962 passed; -p agent-doc-queue-io --lib queue_claim 16 passed; make -k check Rust 11814/11814 (182 skipped), only failure the pre-existing VS Code native.test round-trip (passes 22/22 with a temporary .agent-doc root).
> Follow-ups: (1) live_queue_head_identities still reads only the selected queue for prune/active/claimed_items closure; judge claim closure across every queue. (2) delete-plus-add at the same position still hands over a claim as an edit (#queue-mutation-steering design; decision needed).
<!-- agent-doc-subagent-completion key=c329a8b06f1564a79e4267a1812689fd4bd00be19466436d3a92c634eef29568 -->
> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/380

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/381

### Re: Subagent completed: `subagent:gh381` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/381” — agent-doc
<!-- agent-doc-subagent-response sha256=1a5922840550989cf9daa5b4d0a85ec858ec36b916877bfdbf733c4058f26293 -->
> **Subagent response:**
>
> Branch fix/gh381-project-preset-queue-head, commit 54a4ecf938 (pushed).
> Root cause: preset_only_identity_ids and the preflight Todo audit read only the session document's own frontmatter presets, so a project-only preset head `#review` was treated as tracked id `review`; with an agent:backlog lacking it, head_is_drainable called it a stale mirror (queue_drainable_head_count 0) and append_unlinked_queue_signals flagged a missing Todo backlink.
> Fix: `_with_presets` drain variants in agent-doc-queue/src/queue_continuation.rs fed the merged document+project preset map; new effective_prompt_presets_for_drain (agent-doc-frontmatter-io/src/session.rs, falls back to document presets when project.md is unreadable; grants no authorization). Callers migrated: preflight drain count/supervisor head/skip set, session-check + Codex Stop continuation, supervisor idle watch, controller orphan drain + handoff, route closeout drain, consumption proof, plan. Todo audit skips heads whose leading token is a preset-only name (head_leads_with_preset). Open tracked work with the same name keeps precedence; nothing is written into the document. Specs: project-preset-inheritance.md, 07-orchestration-commands.md, 02-document-format.md, pending-system.md.
> Tests: 3 agent-doc-queue units, 1 preflight-io repro, 1 end-to-end test_cli repro (red before the fix with the exact reported output); 2 source guards updated. cargo test -p agent-doc-queue --lib preset 36 passed; preflight-io repro 1 passed; test_cli repro 1 passed; make -k check Rust 11813 passed (182 skipped), all other targets green except the pre-existing VS Code native.test round-trip (fails only under make's AGENT_DOC_ROOT_CEILING_DIRECTORIES; red on main CI 40449457567).
> Follow-up: fix the VS Code native.test.ts round-trip under make check's root ceiling.
<!-- agent-doc-subagent-completion key=ab5a8a457707abf7e17c6b69fe7b84a8adb7d8881a208dd15194f9e49a3fdbc8 -->
> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/380

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/381

### Re: #gh-fix GH #380 + #381, do [#claimfollowstruck] — opus-5 · 2026-10-27T01:40-04:00

All three fixes are on `main` (`d1155e8943..c0a4e87ed7`, fast-forward) and installed locally with one `make install`. GH #380 and #381 are closed with fix summaries.

| Item | Main commit | Fix |
|---|---|---|
| GH #380 | `149f8ab776` | If a bare `claude`/`codex`/`opencode`/`grok` isn't on the child PATH, the supervisor now looks in the usual install directories (`~/.local/bin`, `~/.claude/local`, npm/bun/volta/cargo bins, Homebrew, …). It launches the binary by that path and adds the directory to the end of the child PATH. When nothing is found, the error lists every directory it searched. |
| GH #381 | `58a41ce8f6` | Every drain check and the preflight Todo audit now read the document presets plus the project presets. A head like `- #review` whose preset is defined only in `.agent-doc/project.md` now drains as that preset, with no backlog item and no Todo warning. A name that is also open tracked work still counts as tracked work. |
| `#claimfollowstruck` | `c0a4e87ed7` | A claim now records its queue, and is matched against that queue's live and struck rows. A struck row ends its claim instead of passing it to the next row, and claims never move across queues. A claim that an earlier version already moved off its struck row is put back on it. That repair skips re-claims and claims stored before this fix. |

**Verification.** I ran `make -k check` on the merged branch. Rust: 11829 run, 11829 passed, 182 skipped. Every other target passed except `dev-harness-test`, where one VS Code test fails: "round-trips owned strings through the built library without crashing". That test already fails on `main` CI (run 40449457567 on `d1155e89`), and all three workers saw it before merging, so it was already failing before these fixes. I've queued it as `#vscodenativeceiling`. Each worker also made sure its new tests fail without the fix. The `#claimfollowstruck` worker went further and broke the fix four different ways on purpose, and each break made its test fail. `make install` replaced the binary and `libagent_doc.so`, marked 6 supervisors and 2 controllers to restart at their next idle point, and reloaded the editor library. No JetBrains restart is needed. CI for `c0a4e87ed7` (run 40456320768) was still in progress when I checked; I didn't wait for it.

**Follow-ups filed:**
- `#vscodenativeceiling`: the VS Code test above.
- `#claimclosureallqueues`: deciding whether a claim's row is closed still looks only at the selected queue.
- `#headlessharnesspath`: headless harness launches can hit the same PATH failure as #380.
- `#claimpanecasrace`: a timing race in a CLI claim test that failed once.
- One Todo for you: when a claimed row is deleted and a different row is added at the same position, should the claim be cancelled instead of moving to the new row?

The worker worktrees and the integration worktree are removed; their branches stay on origin.

### Re: follow-ups #vscodenativeceiling, #claimclosureallqueues, #headlessharnesspath, #claimpanecasrace — opus-5 · 2026-10-27T02:35-04:00

All four follow-ups are on `main` (`cd7ddd3f31..66f7655b54`, fast-forward, on top of another session's `#tmuxguardclaudeuser` push) and installed locally with one `make install`. **`make check` is fully green again:** 11852/11852 Rust (182 skipped), VS Code 225/225 + 4/4, exit 0.

| Item | Main commit | Fix |
|---|---|---|
| `#vscodenativeceiling` | `5f38ddfb47` (+ `66f7655b54` VS Code 0.2.89 bump) | The test was wrong, not the native code. It resolved a file inside the checkout and only passed when some agent-doc project existed above the checkout. Under `make check`'s root ceiling and in a fresh CI checkout the lookup correctly returns null, which also meant the owned strings the test exists for were never freed. It now builds a hermetic temp project and asserts the exact result. |
| `#claimclosureallqueues` | `a4ba26dfaa` | `live_queue_head_identities` (`queue_continuation.rs:1185`) listed only the selected queue, so a claim on a live `agent:loop` row was pruned at closeout while `agent:run` had work. Now it lists live rows across every queue, and struck rows still close their claims. Drain and dispatch still read only the selected queue. Behaviour change (in the spec): a claim whose row has left every queue now closes even when no queue is ready. |
| `#headlessharnesspath` | `78b0554346` | The #380 resolver moved down into `agent-doc-harness` and is shared. Every headless launch (claude send/streaming, codex turns and probes, opencode probes, opencode, grok, junie, and the `cleanup` summarizer) now goes through one `harness_command` helper. It uses the child's actual PATH/HOME, including document `env:` overrides. |
| `#claimpanecasrace` | `11602fde31` | A product race, not a test defect. A provisioning claim registered the pane before launching `agent-doc start`, then re-registered it after launch (`startup.rs:1299`). By then the child owns the actor generation, so the claim either hit `expected 1, found 2` or silently overwrote the child's `Starting` record with `Ready`. The post-launch write is removed. A test-only barrier now makes the interleaving deterministic: without the fix 0/10 pass, with it 60/60 pass under parallel load. |

**Integration notes.**
- The VS Code test change tripped the editor package-generation check (`plugin-version-check`), so I bumped the extension to 0.2.89 the same way as `b2700613ad`. `scripts/check_plugin_versions.py --bump` doesn't support VS Code.
- `make install` replaced the binary and `libagent_doc.so` and installed VSIX 0.2.89. It marked 7 supervisors to restart at their next idle point. No JetBrains restart is needed.
- CI for `66f7655b54` (run 40461614400) was in progress when I checked once.
- All worker and integration worktrees are removed.

**Filed:**
- `#dispatchrebindaudit`: the waiting route path in `dispatch_recovery.rs:79-85` can still re-register a pane after the child is up. Flagged by the `#claimpanecasrace` worker.
- `#statussynccommitrefusal`: the status-sync checkpoint's `agent-doc commit` refuses while binary-owned completion cells are pending, so this session's `main`-moved status edit waited for this closeout.

### Re: main CI regression + do [#dispatchrebindaudit], do [#statussynccommitrefusal] — opus-5 · 2026-10-27T03:28-04:00

Batch 3 is on `main` (`66f7655b54..845441a434`, fast-forward) and installed locally with one `make install`. `make check`: 11861/11861 Rust (182 skipped), VS Code 225/225 + 4/4, exit 0. CI for `845441a434` (run 40466947529) was in progress when I checked once.

**I caused a CI regression in batch 2.** After `#headlessharnesspath`, `harness_command` resolves the harness binary before spawn. Seven codex `build_command_*` tests build a bare `codex` command, so on CI runners (no codex installed) they failed with `harness program codex was not found` (main CI 40461614400). Local `make check` couldn't catch it because this machine has `claude`/`codex`/`grok` on PATH. `2c835e2cc5` gives those tests a fake `codex` on their PATH override; the product behaviour (fail before spawn, listing every directory searched) stays. To check for others hidden behind CI's first failure, I ran the whole workspace with every harness binary shadowed off PATH. Nothing else PATH-dependent fails. The only other failures (4 `native_reload_handoff` tests and one session-check test) also fail under plain `cargo nextest`, with codex on PATH or not, and pass under `make check`.

| Item | Main commit | Fix |
|---|---|---|
| `#dispatchrebindaudit` | `cc7ff96d3d` | Every route rebind after launching a child `agent-doc start`, or after a supervisor restart, now goes through one fenced bind. The callers are `dispatch_recovery.rs:79-85`, `pane_resolution.rs:1733`, `launch_contract.rs:353/479` and `restart_handoff.rs:56`. The bind reads the actor once. If the record already names the pane, it writes nothing, so the owner's `Starting`/`Busy` stands. If a route/sync projection moved the binding, it reclaims the pane, fenced on the generation it read. If an owner committed another live pane, it follows that pane, and fails closed if that pane is gone. Six unit tests cover this, including a hook that fires between the read and the write, plus one isolated live-tmux route test. |
| `#statussynccommitrefusal` | `845441a434` | There were two bugs in a row. First, a snapshot repair treated "current minus pending completion cells" as stale history and reset the snapshot to HEAD, discarding the status edit. Then the commit check read each completion cell's `### Re:` heading as an uncommitted agent response and prescribed `write --commit`. Both now recognise unedited, digest-attested completion cells. The status checkpoint's `agent-doc commit` now commits the status edit together with those cells. That's the same document-only commit the preflight auto-commit makes, which existing tests require. Leaving the cells out instead, as my backlog item worded it, broke those tests and the spec rule that a document-only commit leaves HEAD equal to the current document. |

**Filed (from `#dispatchrebindaudit`):**
- `#predispatchreadyoverwrite`: pre-dispatch rebinds of a running pane still write `Ready` over live owner state (e.g. `pane_resolution.rs:1880` overwrites `Busy`). Needs an audit of whether dispatch admission relies on that write.
- `#syncbindingfence`: about 20 `project_binding_in` callers in `agent-doc-sync-io` have the same unfenced re-read. The generation-only fence also misses a same-generation owner transition.

All worker and integration worktrees are removed. The exchange keeps growing (it was over 800 lines). The queue is draining, so I'm not compacting.

### Re: do [#predispatchreadyoverwrite], do [#syncbindingfence] — opus-5 · 2026-10-27T04:36-04:00

Both are on `main` (`845441a434..947b0c95c3`, two fast-forward pushes) and installed locally with one `make install`. The authoritative `make check` on the final tree passed: 11879/11879 Rust (182 skipped), and 0 VS Code failures. That tree is byte-identical (`e096072668`) to the one the `#syncbindingfence` worker tested. CI for `5defd47734` and `947b0c95c3` was in progress when I checked once. The run for `845441a434` was cancelled because a newer push replaced it. So no CI run has completed on `main` since the batch-3 test fix. I didn't wait for these.

| Item | Main commit | Fix |
|---|---|---|
| `#predispatchreadyoverwrite` | `5defd47734` | The route's own dispatch decision never relied on the `Ready` write; it reads the supervisor's live state. But the controller's `admin queue drain` gate (`actor_busy_draining`, `rpc.rs:27740`) does read the record, and `authoritative_dispatch.rs` bound `Ready` before asking the controller, so a live `Busy` turn slipped through. All 12 pre-dispatch rebinds of a running pane now use `bind_running_dispatch_target_in`. If the record already names this session and pane in a live state, it doesn't write the actor (it refreshes registry metadata only), so `Busy`/`Starting` stand. If a live owner holds another pane, it writes nothing and fails closed. Otherwise it writes a fenced `Ready` bind and re-decides if the fence loses. |
| `#syncbindingfence` | `947b0c95c3` | Production sync-io has 2 binding writers, reached from 4 call sites; the "about 20" count included tests. The window refresh is now window-only and fenced. The recovered-owner rebind keeps the owner when the record already binds it, and otherwise binds fenced. Some owner transitions don't bump the generation (lifecycle state, harness, same-pane reprojection, controller closes, prune, eviction), but every writer moves `documents.last_transition_id`. That id is now an `ActorRevision` write token, and every fenced write, including the post-launch and pre-dispatch binds, compares generation and revision. |

**Integration.** `#syncbindingfence` conflicted with `#predispatchreadyoverwrite` in about 16 hunks of `registration.rs`, because both had replaced the same fence parameter with their own `ActorBindingWrite` enum. I sent the `#syncbindingfence` worker back to rebase. Its variants turned out to be the same as two of the sibling's (`Project` = `Reproject`, `RegistryOnly` = `PreserveOwner`), so the result has three variants. It also extended the revision fence to the pre-dispatch bind and added a same-generation race test for it. Disabling the revision check fails exactly the three same-generation tests.

**Filed.** To the backlog, so they go to the go queue:
- `#controllerclosefence`: the controller-io stale/dead-pane closes are fenced on generation only.
- `#bindingcallersaudit`: the binding writers in preflight-command-io and `authoritative_actor.rs` haven't been audited.
- `#drainbusymask`: a stale `Busy` is now visible to the drain gate until reconcile heals it. This is a side effect of `#predispatchreadyoverwrite`.
- `#prunetestisolation`: 4 ignored `purge_*` tests fail on machines that have `~/.agent-doc/state.db`.

To the icebox, as speculative and not queued: `#predispatchblockedaudit`, which asks whether `Blocked`/`Closed` same-pane records should also be preserved, and covers the stale `window_id` after a stash rescue.

The worker and integration worktrees are removed; the branches stay on origin.

### Re: do [#prunetestisolation], [#drainbusymask], [#controllerclosefence], [#bindingcallersaudit] — opus-5 · 2026-10-27T05:28-04:00

All four are on `main` (`947b0c95c3..4e17a4c793`, fast-forward) and installed locally with one `make install`. `make check` on the merged branch: 11883/11883 Rust (182 skipped), VS Code 0 failures, exit 0.

| Item | Main commit | Fix |
|---|---|---|
| `#prunetestisolation` | `aa6adb907f` | All 7 `purge_*` tests failed on this machine, not the 4 reported earlier. Their panes started in the checkout, and the purge searched up for the nearest `.agent-doc/`, reaching `~/.agent-doc/state.db`. Each test now runs in its own temp project and asserts that every pane resolves there. The ignored sync-io suite is 124/124. |
| `#drainbusymask` | `3179610e4f` | This was a wedge, not a delay. The supervisor's idle-pane reconcile marked a repair as done before writing it. So a failed write was never retried, and a second `Busy` stretch with no new dispatch was never repaired. During `admin queue drain` the document stayed `actor_busy_draining` until `admin queue resume`. Now an unlanded write is retried and every busy stretch gets its own repair. The route and the drain gate are unchanged, so a live `Busy` is still blocked. The route's own pane evidence was rejected as too weak to promote the actor, because it can see a live turn between tool calls. |
| `#controllerclosefence` | `18c347c737` | Five controller sweeps (stale-`Starting` close, dead-pane/idle close, cross-document eviction, the start_session stale-alias close, and the prune delete) now decide from one read of the record and its revision. They write fenced on generation + revision and re-decide when the fence loses. The prune had no fence at all: a start that re-opened the document after the decision had its new actor and lease deleted. |
| `#bindingcallersaudit` | `5406b8ba70` | The two sites named in the item were test fixtures. The real gap was three route lifecycle writes in `authoritative_actor.rs`, fenced on generation only. One could write `Ready` over the owner's `Busy`, which also released the running turn's dispatch receipt. One could revive an owner-closed actor. One could mark an owner `Blocked` after it came up. All three now go through `mark_route_lifecycle_fenced`. The revision travels to the controller in `LifecycleRequest::expected_revision`; owner and operator callers pass `None`. |

**Integration.** Three spec files conflicted, and I kept both rules in each. The code merged textually, but the build broke: `#drainbusymask`'s new controller test builds a `LifecycleRequest`, and `#bindingcallersaudit` added the `expected_revision` field. `4e17a4c793` sets it to `None`, as every other owner caller does.

**CI.** No run has finished on `main` since the batch-3 test fix. Each new push cancelled the previous run (`845441a434` and `5defd47734` were cancelled). `947b0c95c3` (run 40474332523) and `4e17a4c793` (40480622471) were in progress when I checked once.

**Filed.** I'm sending only concrete bugs to the go queue now; speculative findings go to the icebox.
- Backlog: `#syncnorootfallback`. Sync's no-project-root fallback writes an unfenced actor record even when called with `may_write_actor=false`.
- Icebox: `#ownerlifecyclerevive` asks whether the owner's own `Ready` should refuse to revive a `Closed` actor. It also covers the unverified durable-`Busy`/in-memory-`Ready` drain case. `#evictdeadcode` covers `evict_cross_document_pane_bindings`, which has no callers.
- Todo for you: should `~/.agent-doc/` make `$HOME` a project root? Today any pane under `$HOME` with no closer `.agent-doc/` resolves to `$HOME`.

The worker and integration worktrees are removed.
<!-- agent:boundary:d7b4ac5d -->
<!-- /agent:exchange -->

<!--
-->

## Run <!-- agent:run subagents preset="#spec-test-build-install-commit-push" priority go -->
- do [#syncnorootfallback]
- ~~do [#controllerclosefence]~~
- ~~do [#bindingcallersaudit]~~
- ~~do [#drainbusymask]~~
- ~~do [#prunetestisolation]~~
- ~~do [#predispatchreadyoverwrite]~~
- ~~do [#syncbindingfence]~~
- ~~do [#dispatchrebindaudit]~~
- ~~do [#statussynccommitrefusal]~~
- ~~do [#vscodenativeceiling]~~
- ~~do [#claimclosureallqueues]~~
- ~~do [#headlessharnesspath]~~
- ~~do [#claimpanecasrace]~~
<!-- /agent:run -->

## Loop <!-- agent:loop interval="1h" priority -->
- 📥 Watch https://github.com/btakita/agent-doc/issues for new issues. Add new issues into the backlog + queue except issue #40 + #45.
<!-- /agent:loop -->

## Todo <!-- agent:todo priority -->
- [ ] [#ahfe] Live-check GH #267 on the 263 IDE: after `agent-doc upgrade` WITHOUT restart, once idea.log shows `[native] hot-reloaded libagent_doc`, Run Agent Doc should complete (ops.log `editor_route_superseded_editor_admitted`, route output ends with the advisory line). 2026-10-15 operator: did not work (believed); watch for bug reports before reopening GH #267.
- [ ] [#ctg7] Under heavy load, switch the editor 2 panes -> 1 -> 2 and confirm tmux follows and stays at 2 (stale-controller fix).
- [ ] [#coder-remote-install-02805] Repair Coder Remote Dev sync after the two-sided install: open /home/brian/work/btakita/agent-loop as the backend IDE project root; ensure Client/Gateway and backend both load JetBrains 263 plugin 0.2.824; restart both; on the backend run agent-doc env --json, agent-doc tmux ensure tasks/agent-doc/agent-doc-bugs.md --ide-terminal --json, and agent-doc session status tasks/agent-doc/agent-doc-bugs.md; require client idea.log to show snapshot schema negotiated with geometry=measured, backend idea.log to show accepted ingress with no DELIVERY_FAILED, and .agent-doc/logs/ops.log to show controller_editor_surface_observed client=jetbrains-rd@... surface=remote-client:... followed by pane_layout_desired_published with column_order=editor. If DELIVERY_FAILED No such file or directory persists, fix ControllerSurfaceSnapshotPublisher to resolve the controller from each session document root instead of an unrelated IDE project basePath, expose connected-without-accepted-publication as blocked while preserving the no-guessing fence, and add a regression test.
- [ ] [#gh379-remotedev-proof] Close GH #379 (https://github.com/btakita/agent-doc/issues/379) after proof from its Remote Dev setup (Windows JetBrains Client): on that backend, `agent-doc --version` is 0.35.555 or newer and the IntelliJ IDEA 2026.3 plugin is 0.2.835 or newer; Run Agent Doc then ends exit 0 and ops.log shows `controller_editor_route_measured_layout_resolved` with no `editor route requires the exact visible layout before dispatch`.
- [ ] [#lazily-stale-cell-repair] After the next tasks/software/lazily.md cycle commits it clean (no uncommitted edits, pane prompt_ready=true), run `agent-doc queue repair-stale-completion tasks/software/lazily.md` (dry run, 14 stale completion cells as of 2026-10-26), then `--apply` and `agent-doc commit tasks/software/lazily.md`.
- [ ] [#vscodecodersmoke] Run scripts/vscode-remote-smoke.py on a real Coder workspace (procedure in runbooks/coder-workspace.md): install, register, remote extension host, tmux attach, remoteName, layout sync; then update the VS Code on Coder status cell in editors/SPEC.md section 13.
- [x] [#12t8] Decide whether deleting a claimed queue row while adding a different row at the same position should cancel the claim instead of handing it to the new row as an edit (current #queue-mutation-steering design; raised by the #claimfollowstruck fix). (source: agent:backlog [#claimfollowstruck])
- [ ] [#krwr] Decide whether deleting a claimed queue row while adding a different row at the same position should cancel the claim instead of handing it to the new row as an edit (current #queue-mutation-steering design; raised by the #claimfollowstruck fix).
<!-- /agent:todo -->

## Backlog <!-- agent:backlog priority queue -->
- [ ] [#syncnorootfallback] agent-doc-sync-io/src/sync.rs:6855 rebind_recovered_owner_pane's no-project-root fallback (sessions::register) writes an unfenced actor record even when called through refresh_registry_for_actor_pane with may_write_actor=false; make it registry-only when may_write_actor=false and fenced on generation + ActorRevision otherwise.
<!-- /agent:backlog -->

## Review <!-- agent:review -->
- [/] [#jetbrainsmarketplaceprofile] [marketplace-approval] [agent-doc] Plugin 35436 is hidden and under initial JetBrains moderation; after approval, replace the basic description with polished copy, add Getting Started, Documentation, and Bugtracker URLs, then verify the public listing.
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically by a human reply on pypi/support#12214, or make pypi-quota-check reporting a ceiling in reach. Rechecked 2026-10-12: pre-cutover 131 releases 8.361 GiB (0.35.113..0.35.343), post-cutover 91 releases 3.19 MiB (mean 35.9 KiB), about 47,722 further releases fit; VERDICT headroom fine. The support issue is still open with one comment, unchanged since 2026-09-10. Nothing to act on or delete. If the grant lands, record it and drop this item; deletion stays reserved for a ceiling the authenticated check reports.
- [/] [#gh299-live-263-proof] Live-verify v0.35.519 on JetBrains 263.7031 with changed plugin sources: `agent-doc upgrade` must hot-reload and complete without an IDE restart; close GH #299 only with that proof.
<!-- /agent:review -->

## Icebox <!-- agent:icebox -->
- [ ] [#ownerlifecyclerevive] The owner lifecycle writer (store_lifecycle_actor_record_for_runtime callers, agent-doc-controller-io rpc.rs ~27253) decides from the runtime actor record and writes on generation only, so a same-generation Closed landing in that window could be overwritten by Ready. It is an owner write (kept generation-only by #controllerclosefence/#bindingcallersaudit); decide whether it should refuse to revive a Closed actor. Also unverified from #drainbusymask: a durable Busy with in-memory Ready would still block an admin drain (no writer found that produces it).
- [ ] [#evictdeadcode] evict_cross_document_pane_bindings (agent-doc-controller-io/src/project_controller.rs, fenced by #controllerclosefence) has no callers in the workspace; remove it or wire it.
- [ ] [#predispatchblockedaudit] After #predispatchreadyoverwrite, a Blocked or Closed record on the same pane, and a different-session record on the same pane, still get the pre-dispatch Ready rebind; audit whether a Blocked owner or the route's starting-timeout Blocked latch should be preserved. Related: after a stash rescue the record's window_id stays stale until sync's (now fenced, window-only) refresh repairs it.
- [ ] [#jbropeshadow] Make the JetBrains replica shadow a non-String text (rope or piece table) in CrdtReplicaManager and CrdtReplicaForwarder (knownReplicaText, the next flush base), so a flush stops allocating and copying the whole next shadow; that copy is now the only document-size-dependent part of prepareLocalEditorEditsUtil (about 25 us at 125 KB, 177 us at 500 KB). Gate on make jetbrains-keystroke-bench.
- [ ] [#jbfrontendswap] [agent-doc] Remote Dev frontend implementation swaps for the GH #339 shell design: let the JetBrains Client load the swappable implementation from the backend so frontend changes stop needing a restart. Parked 2026-10-26: it pushes backend code into the local client JVM without a plugin install, which changes the security boundary. The frontend stays shell-only (editors/jetbrains/SPEC.md, "Restart-free implementation generations").
- [ ] [#ghrecvpolicy] [agent-doc] Recipient-side [receive] auto_create_components policy for multi-user workspaces (GH #40, https://github.com/btakita/agent-doc/issues/40). Parked until multi-user workspace support exists.
<!-- /agent:icebox -->

## Done <!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
