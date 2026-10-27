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

Main is at `b6b9af6e15` and agent-doc 0.35.532 is installed locally. Release v0.35.532 was published from `71f9801b49`. Post-release main now includes the inline-code-safe, occurrence-ordered queue preset authorization fix for GitHub issue #331 and the positive-evidence-only Remote Dev pane-order fix for GitHub issue #332; both issues are closed, the integrated full suite passed, and the current mainline was installed locally.

<!-- status:sync -->
Released and published agent-doc v0.35.556 from release commit `5cbfdbbc4c`; tag `v0.35.556` is the stable latest release (GitHub Latest, PyPI 0.35.556, stable target channels). JetBrains classic/262/263 0.2.839/0.2.841/0.2.840, VS Code 0.2.83. It adds onboarding Getting started, the queue marker legend, the component-marker fences (live edit, cold start, JetBrains edit-time warning), row-aware editor attach, reliable echoed-head strikes beside a loop queue, doctor/autofix exit codes, and pane-process launches for start/route/fan-out. GH #378 closed; GH #379 stays open pending Remote Dev proof (todo `#gh379-remotedev-proof`). Post-release `origin/main` is `9aca9eec10` (unreleased, installed locally; JetBrains classic/262/263 0.2.845/0.2.846/0.2.847 need an IDE restart): the in-IDE restart-required prompt, no-argument `/agent-doc` on all four harnesses via `agent-doc document`, `agent-doc queue repair-stale-completion`, id-less `agent:run` / `agent:loop` named by kind (`run` / `loop`), the GH #376 measured route publishing after its own surface Sync, the GH #339 restart-free shell design (spec + test-only spike), the JetBrains keystroke-latency benchmark with route-outcome and exchange-prefix-repair moved into the hot-reloaded native library, todo `id=` always winning over a cited tracked id, the editor frontend/backend parity matrix (editors/SPEC.md section 13), and a test + spec pinning that the done/backlog free-text strike skips claimed rows.
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
<!-- agent:boundary:3ec64169 -->
<!-- /agent:exchange -->

<!--
-->

## Run <!-- agent:run subagents preset="#spec-test-build-install-commit-push" priority go -->
- 🚧 [#subagent:jbshellr1rest#1] do [#jbshellr1rest]
- 🚧 [#subagent:vscodesession#1] do [#vscodesessionparsers]
- 🚧 [#subagent:replicaremote#1] do [#replicaremoteflush]
- 📥 do [#kotlinflushprep]
- 🚧 [#subagent:vscodekoffi#1] do [#vscodekoffivsix]
- 🚧 [#subagent:vscodesegv#1] do [#vscodekoffisegv]
- ~~do [#vscoderemoteparity]~~
- ~~sdk-demo.md turn did not complete. Response is uncommitted.~~ — auto-struck: answered this cycle (#ftstrike)
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
<!-- /agent:todo -->

## Backlog <!-- agent:backlog priority queue -->
- [ ] [#jbshellr1rest] Finish GH 339 shell stage R1 in one worker, one commit per step, in this order: (1) forward project lifecycle and the programmatic listeners through the shell and retarget generateModularLifecycle; (2) shell forwarders for the editor/language extensions; (3) GenerationContext.invokeLater with FencedTask plus per-generation executors and CoroutineScope for EDT and pooled work; (4) controller identity: the activation_receipt field on the Rust EditorRegistration (serde default) and the Kotlin Register op, landed together (VS Code already added an optional activation_receipt field on main); (5) shell-created Keys and shell-side Fleet RPC delegation, moving AuthenticatedSurfaceIngressState out of the service file. Implementation stays in the same classloader, frontend shell-only.
- [ ] [#vscodesessionparsers] Point the VS Code extension clear/restart parsers (editors/vscode/src/sessionUi.ts) at the native session_command_outcome derivation behind agent_doc_editor_derive_json, with the TypeScript copy as the frozen fallback answering the shared session_command_outcome.json fixture identically.
- [ ] [#replicaremoteflush] Fold remote CRDT ops into the replica visible-order index as they arrive (agent-doc-merge crdt_sync local_ops), so the first local flush after a remote update no longer pays a full index rebuild; keep the differential tests against lazily paths and gate on make jetbrains-keystroke-bench.
- [ ] [#kotlinflushprep] Make the JetBrains prepareLocalEditorEditsUtil step proportional to the edit: it is now the size-dependent part of a replica flush (about 30 us at 125 KB, 170 us at 500 KB); gate on make jetbrains-keystroke-bench.
- [ ] [#vscodekoffivsix] Ship koffi in the VS Code VSIX: editors/vscode/.vscodeignore excludes node_modules, so a VSIX install has no koffi and the native library is off (nothing registers with the controller, including on Coder); bundle or include the koffi runtime for each target platform and add a packaging test that the VSIX contains it.
- [ ] [#vscodekoffisegv] Investigate the VS Code extension-host SIGSEGV in free called from koffi into libagent_doc under code-server 4.143.1 (node v24.22.1), reproduced with the debug library on unmodified main by the vscoderemoteparity worker; find the ownership bug at the FFI boundary and add a regression test.
<!-- /agent:backlog -->

## Review <!-- agent:review -->
- [/] [#jetbrainsmarketplaceprofile] [marketplace-approval] [agent-doc] Plugin 35436 is hidden and under initial JetBrains moderation; after approval, replace the basic description with polished copy, add Getting Started, Documentation, and Bugtracker URLs, then verify the public listing.
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically by a human reply on pypi/support#12214, or make pypi-quota-check reporting a ceiling in reach. Rechecked 2026-10-12: pre-cutover 131 releases 8.361 GiB (0.35.113..0.35.343), post-cutover 91 releases 3.19 MiB (mean 35.9 KiB), about 47,722 further releases fit; VERDICT headroom fine. The support issue is still open with one comment, unchanged since 2026-09-10. Nothing to act on or delete. If the grant lands, record it and drop this item; deletion stays reserved for a ceiling the authenticated check reports.
- [/] [#gh299-live-263-proof] Live-verify v0.35.519 on JetBrains 263.7031 with changed plugin sources: `agent-doc upgrade` must hot-reload and complete without an IDE restart; close GH #299 only with that proof.
<!-- /agent:review -->

## Icebox <!-- agent:icebox -->
- [ ] [#jbfrontendswap] [agent-doc] Remote Dev frontend implementation swaps for the GH #339 shell design: let the JetBrains Client load the swappable implementation from the backend so frontend changes stop needing a restart. Parked 2026-10-26: it pushes backend code into the local client JVM without a plugin install, which changes the security boundary. The frontend stays shell-only (editors/jetbrains/SPEC.md, "Restart-free implementation generations").
- [ ] [#ghrecvpolicy] [agent-doc] Recipient-side [receive] auto_create_components policy for multi-user workspaces (GH #40, https://github.com/btakita/agent-doc/issues/40). Parked until multi-user workspace support exists.
<!-- /agent:icebox -->

## Done <!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
