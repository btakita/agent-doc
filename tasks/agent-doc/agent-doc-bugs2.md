---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
resume: e61edbb1-dd19-402f-9bed-69d29c9a1a2c
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
pending_done_guard: warn
agent_doc_lint_dialect: off
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog + queue items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find all contributing factors to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#simworld': Strengthen Simworld tests.
  '#spec-test-build-install-commit-push': red-green-refactor. update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': red-green-refactor. update spec + tests. commit + push
queue: go
---

## Status

<!-- agent:status patch=replace -->

completed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260806-204403.md`*

Compacted content:
- Archived 3 response topic(s): tmux auto-sync delay; do [#tmuxautosyncreactive]; do [#tmuxautosyncreactive]
- Prior summary/context: prior compacted content: Archived 2 response topic(s): Agent Doc recovery and root-cause fixes; Response

### Re: tmux auto-sync reactive conversion — glm-5.2

> **Queue prompt:**
>
> do [#dc0c]


> **Queue prompt:** The tmux pane auto-sync is still slow. Please continue optimizing the performance of the tmux pane auto-sync. Convert imperative logic to reactive state. Also, fix any CRDT storms and/or feedback cycles in the reactive state.

**Evidence-driven triage.** The `/tmp/agent-doc-sync.log` phase meter shows the headline editor tab-switch wedge is already resolved by the `#tmuxautosyncreactive` work shipped through 0.35.157: `handle_editor_surface_observe` (`rpc.rs:17059`) now folds against the **retained** tmux observation and resolves `Sync`/`Focus` in-process — the editor socket no longer blocks on a tmux survey or a structural sync (plan item 1 main, DONE). The sync lock is uncontended (0 ms), and the editor-visible path skips the `/proc` ownership walk via the reactive actor-binding fast path. The remaining per-phase cost (`ownership_per_file_loop` ~1.2 s) belongs to **full CLI syncs** (`exact_visible_projection=false`), not the editor tab-switch path, and shortcutting it there would regress the `#bare-foreign-session-guard` / cross-repo owner proofs.

**Shipped this cycle (0.35.158).** The one remaining steady-state reactive violation on the editor hot path: `handle_tmux_focus_state` reopened `state.db` and loaded the full actor store on **every 500 ms** JetBrains reverse-mirror poll (`#lazily-hot-path`: SQLite is a durable sink, not transition authority). The RPC is now routed through the runtime like its siblings and reads `ControllerRuntime::actor_store_snapshot()` (the in-memory reactive store); cold start still hydrates from SQLite. The live pane→record lookup is unified in a pure `actor_record_for_active_pane` helper (new unit test). Verified: `make check` green, `make install` done, pushed.

**On "CRDT storms / feedback cycles".** The sync path does not write to CRDTs (closeout owns that), and the `editor → surface graph → worker → tmux → surface graph → editor` loop is already guarded by identical-invocation dedup (`set_desired`), stale/duplicate observation rejection (`ControllerEditorSurfaceGraph::observe`), single-worker coalescing (`LatestProjectionWorkerState`), and generation supersession. The 500 ms poll was the last contributor feeding that loop; this cycle removes its SQLite read.

**Item 2 re-framed.** The original plan suggested replacing the JB 500 ms poll with an editor-surface state-plane subscription. That projection carries the editor→tmux `intent`, **not** the tmux-active document, so it cannot drive the reverse mirror — and tmux exposes no push event for arbitrary pane-focus changes, so the active-pane probe must stay. The achievable reactive conversion is the one landed above (serve the poll from the reactive store). Plan updated (`plan-tmux-auto-sync-reactive.md`) with a note explaining why a pure-subscription replacement is out of reach until a tmux focus-change hook exists.

**Closed `#dc0c`** as a duplicate of the shipped `#jb-tsift-pane-sync` (semantic match 0.883) — same "navigation did not surface the pane; manual Sync Tmux Layout did" class, already fixed.

**Remaining (low priority).** Item 4 (`ControllerEditorSurfaceGraph` `Mutex<BTreeMap>` → `ThreadSafeDependencyMap`) is now low-value since item 1 main moved the consequence off the editor-observe path, so the mutex is no longer held across expensive work. Left in the plan, deprioritized.

### Re: do [#opencodeharnessswitch] — opus-5

**Reproduced, root-caused, fixed, shipped as 0.35.162.** The backlog item asked for a fresh reproduction with pane-capture evidence to decide between "TUI init exceeds 30s" and "the composer projection returns `Absent`/`Busy` instead of `ReadyEmpty`". It is the second one, and the pane reached that state in about 3 seconds — the 30s deadline was never the constraint.

**Evidence.** I ran OpenCode 1.18.14 in an isolated tmux server and captured the idle cold-start splash. Two properties of the real screen were absent from the hand-written 1.14.x fixtures in the test suite:

1. The renderer leaves **~16 blank rows** between the composer box and the bottom `<cwd>:<branch> … <version>` status line. `is_bottom_idle_chrome` bounded its window with `.lines().rev().take(12)` over **raw** rows, so the whole window went to padding and exactly **one** chrome line survived — under the OpenCode `min_chrome` floor of 4. The sibling `recent_normalized_bottom_lines` already pops trailing blanks for the busy-cue window; this path never got that treatment.
2. The `opencode` wordmark is drawn with `█` (U+2588), which was missing from the box-art glyph set. Those rows were therefore non-ignorable, so the all-lines `is_idle_chrome_only_output` scan also failed and `last_prompt_candidate` resolved to a wordmark row.

With both paths failing, `project_pane_composer` fell through to `Absent`, `pane_ready_prompt_candidate` returned `None`, and the auto-trigger logged `auto_trigger_timeout harness=opencode reason=no_prompt_after_30s`.

**Fixes.** The window is now bounded in *meaningful* rows, and one shared `scan_bottom_idle_chrome` serves both the readiness consumer and the stale-busy-cue consumer so the two window definitions cannot drift (they were duplicated, and both carried the bug). `█` joined the box-art set, which still requires *every* non-whitespace character to be an art glyph.

**A hazard the widening introduced, caught by capturing a drafted composer too.** OpenCode renders its idle chrome — box rule, `Build ·` plan line, keybinding hints, cwd/version status — *below* the composer. So chrome alone reaches `min_chrome` before the scan ever reaches the composer row. Once padding stopped truncating the window, a composer holding `┃  hello` reported **idle**, and route would have injected a trigger over unsent operator text. A composer row carrying operator text is now an explicit reject. Only the padding bug had been hiding this.

Verified against four live captures — cold-start splash (plain and ANSI), focused-idle, and drafted — through all three consumers (`output_prompt_visible`, `project_pane_composer`, `ready_prompt_candidate_at_cursor`): the three idle states are dispatch-ready, the drafted one fails closed.

**Historical note.** The `no_prompt` entries in `src/boost-client/.agent-doc/logs/` also show an *older*, separate cause: the capability proof ran the **Codex** probe shape against opencode, which printed opencode's help screen and exited nonzero. That one is already fixed — `prove_managed_session_capabilities` now branches to `prove_opencode_child_network_access`.

### Re: tmux pane auto-sync excessive lag — opus-5

> **Queue prompt:**
>
> do [#opencodeharnessswitch]


**Measured first.** From `/tmp/agent-doc-sync.log`, since 0.35.158 the editor path is median **1.0s** wall, p90 2.0s — but the metered phases only account for ~520ms of it. The gap was two **unconditional** full layout-repair passes, one before and one after the reconciler, and they were invisible to the phase meter because they sat *between* measured phases.

The log proves they never converged: **2764** pre-sync and **2761** post-sync `Repaired \`agent-doc\`/\`stash\` layout` claims, on essentially every sync, forever. `repair_file_state_with_tmux` pushed that note unconditionally, so it was indistinguishable from a repair that genuinely never settles.

`repair_layout` already had a fast path for phases 1+2, but phases 3+4 were marked "always runs" — and each one re-lists the session's windows: `consolidate_duplicate_target_windows` (1), the stash index plan (1), plus one more *inside every* `normalize_window_to_index` call. An editor tab switch was paying roughly a dozen redundant tmux round-trips per gesture to rediscover a layout that was already correct.

**Fix.** A pure predicate, `session_layout_normalization_converged`, evaluates the entire phases-3+4 plan against **one** window observation. That is exact for the converged case, because a converged layout is precisely one where no phase moves anything and therefore no index shifts under the later phases. When it holds, `repair_layout` returns `AlreadyConverged` and issues no further tmux command; otherwise it falls through to the existing code and returns `Repaired`. Callers report a note only for a real repair.

Both passes are now metered as `doctor_repair_pre` / `doctor_repair_post` against a 250ms budget, so the next live syncs will show the improvement rather than me asserting it — that is a backlog item, and it needs no human eyeball.

**What I did not change.** `ownership_per_file_loop` still averages 345ms with a 13.3s max, but the log shows that cost belongs to **manual/CLI full syncs** where `exact_visible_projection=false` (1231ms) — not to editor tab switches, which reuse the reactive actor binding and land at ~90ms. That matches the prior cycle's finding and stays deprioritized.

**Shipped.** `make check` green, `make install` done, `agent-doc admin recycle` on both project roots, committed and pushed (agent-doc `2e8ce91b6`, superproject pointer `3729245e64`). CI run 31234381764 was still queued at closeout.

### Re: do [#runfilesubmit] — opus-5

**Reproduced from the preserved capture, root-caused to two defects, both fixed and shipped as 0.35.163 / JB 0.2.350.**

**The evidence.** `.agent-doc/logs/route-submit/1786163221429-idle_queue_payload_observation-claude-_25-4b7d177d1f91.txt` is the pane, captured 429ms after route reported success. Its last four lines:

```
❯ agent-doc /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md

  Opus 5 ~/work/btakita/agent-loop main brian@cachyos-x8664
  ⏵⏵ bypass permissions on (shift+tab to cycle)
```

The trigger is drafted, the pane is idle, no turn started. Meanwhile ops.log for the same second: `route_latency phase=direct_pane_submit elapsed_ms=4 status=ok outcome=pass_through_single_submit` and `controller_editor_route_settled … exit_code=0`. Route was reporting success at a submit that never crossed the composer.

**Defect 1 — the pass-through path never looked.** `DirectPaneSubmitPolicy::PassThroughSingleSubmit` exists so a plain `agent-doc <FILE>` trigger returns immediately instead of paying the dispatch-start proof budget. It did that by calling `send_command_once_checked` and returning `TransportSubmittedOnly` — one `tmux send-keys` with text and `Enter` in the same call, then nothing. When the harness TUI absorbed that trailing `Enter` into the same input burst, there was no observer to notice. It now verifies the draft cleared and repairs it with a bounded bare submit key.

**Defect 2 — and the predicate that should have caught it was blind.** `route_trigger_visible_in_current_draft` decides "is this trigger the live draft, or consumed scrollback?" by asking whether a prompt line appears *after* it. Claude Code renders its permission-mode footer **below** the composer, and `HarnessConfig::is_prompt_line` matches `⏵⏵ … (shift+tab to cycle)` on purpose — it is a legitimate idle cue elsewhere. So that footer terminated the scan on every single capture, and the predicate answered `false` for a trigger sitting in plain sight.

This is the deeper one: it silently disabled the visible-draft `Enter` recovery *everywhere*, not just on the new path — `try_late_direct_pane_enter_resubmit_after_unproven_dispatch` and `direct_pane_can_enter_existing_draft` were dead for Claude too. Persistent status-footer chrome is now excluded from the later-prompt scan; a real composer prompt below the trigger still ends it, so consumed scrollback is still told apart. The regression test replays the verbatim capture above and was red before the fix.

**What the repair will not do.** It presses a key only when the trigger is the visible draft on an **idle** pane, only after the sighting survives a settle window (one frame of render lag is not a stranded draft), and at most 3 times. A pane-capture failure is never read as "stranded" — unknown pane state authorizes nothing. When the submit crossed, the whole check is one `capture-pane` with no settle, so the fast path stays fast.

Every pass-through submit now records `route_pass_through_submit_draft … outcome=cleared|deferred_pane_busy|enter_resubmit|exhausted_still_stranded enters_sent=N`, which makes the stranding rate measurable instead of invisible.

**On the semantic-completion warning.** Preflight flagged `#runfilesubmit` as a possible duplicate of `#0dsr` (0.939). It is not: `#0dsr` is about Run Agent Doc recovering *any* incomplete operation after a failure. This was a submit key swallowed on the way in — nothing incomplete to recover, because nothing ever started.

**What I did not change.** Codex and OpenCode split the text and submit key by 80ms precisely because a same-call `Enter` gets swallowed. Claude plausibly needs the same prevention, but I have proof of the *symptom*, not the mechanism, so I fixed it where the proof is and left the submit profile alone. The new log line is the measurement that decides it — filed as `#passthroughsplitprofile`.

### Re: the long queue-continuation prompt — opus-5

You asked for this twice — as a queue item and again mid-turn for `haiven-dev.md`. Removed in the same release (`#qcontprose`).

The supervisor's idle-queue watch was typing this into your console on every head advance:

```
Agent Doc queue state advanced for `tasks/haiven-dev.md`. Continue this existing owner-pane
session; do not invoke `agent-doc` recursively.

Execute the current active queue head:

rebasemsgspecbranch

Persist and finalize this cycle through the connected Agent Doc tools.
```

It now sends `agent-doc <file>` — the same bare trigger the route path has always dispatched into live owned panes.

The recursion the paragraph warded against does not exist. In an owned pane the *harness* is the process reading input, so `agent-doc <file>` is a prompt its `UserPromptSubmit` hook admits and answers with an in-binary preflight; the same session continues. This very cycle started that way. Naming the head was redundant too — preflight already selects it and hands it over as `selected_queue_prompts`. `agent_doc_queue::idle_drain`'s own contract test had asserted the payload should be `agent-doc <file>` all along; only the caller disagreed. Two dispatch paths that disagreed about what a queue continuation looks like are now one.

### Re: keep the last agent-doc file when the editor moves to source — opus-5

Fixed as `#stickymdpane` in JetBrains plugin 0.2.350.

Both surface reporters answered "which document is this editor window showing?" with `window.selectedFile?.takeIf { it.name.endsWith(".md") }`. Open a source file in one half of a two-column split and that window reported *nothing* — so the mirrored layout collapsed to a single tmux pane, and the session pane you were working next to was gone until you navigated back to the document.

A window that has shown a document now keeps standing for the last one it showed, resolved from that window's own `.md` tabs in `EditorHistoryManager` most-recently-used order. Using the IDE's own selection history means no plugin-side per-window bookkeeping, so it survives restarts and splits. A live `.md` selection always wins over the remembered one, and a window that never held a document still contributes no pane — closing a split, or opening a fresh source-only one, collapses the layout exactly as before. The fallback is memory, not invention.

This one needs your eyes: it is Kotlin, so it takes an IDE restart to load. Filed as `#stickymdpaneverify`.

### Re: shipping — opus-5

> **Queue prompt:**
>
> do [#qcontamquote]


`make check` green (**8689 tests passed**, 248 skipped), `make install` done, `agent-doc admin recycle` on both project roots, committed and pushed (agent-doc `19d993e11`, superproject pointer `f0818b7de3`). CI run 31241516366 was still pending at closeout.

**Two things about the working tree you should know.** A concurrent session is mid-edit on `agent-doc-sync-io/src/sync.rs`, `sync/pane_repair.rs`, `agent-doc-watch-io/src/{daemon,lib}.rs` and an untracked `tasks/agent-doc/plan-sync-slow-path-latency.md` — that looks like the queued "tmux pane auto-sync is still extremely slow" item. I left all five uncommitted. But the `release-version` projection rewrites *every* manifest, so `agent-doc-watch-io/Cargo.toml` and `agent-doc-sync-io/Cargo.toml` had to go in, and `agent-doc-watch-io/Cargo.toml` carried that session's new `libc = "0.2"` line with it. Skipping those two manifests would have left HEAD declaring 0.35.162 crates against 0.35.163 requirements and unbuildable, so I took the one-line contamination over a broken HEAD. Their source still belongs to them.

I also left the queue item that folded "tmux pane auto-sync is still extremely slow" and the long-prompt report into a single node untouched rather than striking it — striking would have consumed the auto-sync half, which is in flight elsewhere.

### Re: CI went red on my own commit — opus-5

Correcting the closeout above: CI run 31242159018 failed, and it was my fault, not the concurrent session's.

`tests/test_cli.rs` is one shared architecture-guard file that every session appends to. I committed it wholesale, so it carried that session's in-progress `#stalewatchpid` assertions against `agent_doc_watch_io::{clear_stale_pid, clear_stale_pid_in, pid_is_agent_doc_watch}` and the fallible `remove_pid` signatures — all defined in their still-uncommitted `agent-doc-watch-io/src/lib.rs`. HEAD did not build (E0425 ×2, E0308 ×2).

**`make check` could not have caught this.** It ran against *my* working tree, which has their `lib.rs`. 8689 tests passed locally on a HEAD that does not compile. Local green is not evidence about HEAD when another session has uncommitted source in the same tree.

Reverted only their hunk at HEAD (`6d1c64167`, superproject `5778d3129c`) and restored it to the working tree uncommitted, so those assertions land with their source when they commit. `git diff a739c20b6 HEAD -- tests/test_cli.rs` is now exactly my two `#runfilesubmit` guard snippets and nothing else. CI run 31242298646 was pending at closeout.

The manifests are different and had to stay: `make release-version` rewrites all 144 of them, so a version bump necessarily stages ones another session touched, and omitting them would leave HEAD declaring 0.35.162 crates against 0.35.163 path requirements — also unbuildable. The `libc = "0.2"` line rides along; their source does not.
<!-- agent:boundary:9a949f6b:agent-doc-bugs2 -->
<!-- /agent:exchange -->
## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- ~~do [#qcontamquote]~~
- do [#passthroughsplitprofile]
- ~~do [#qcontprose]~~
- ~~do [#stickymdpane]~~
- ~~do [#runfilesubmit]~~
- do [#syncdoctorverify]
- tmux pane auto-sync is still extremely slow when switching from lazily.md to agent-doc-bugs2.md. Fix it.
---
The prompt for Claude Code should simply be `/agent-doc .../file.md`. There is a long prompt instead:
```
❯ Agent Doc queue state advanced for `tasks/software/lazily.md`. Continue this existing owner-pane session; do not
  invoke `agent-doc` recursively.
  Execute the current active queue head:
  lzcoveragefamily
  Persist and finalize this cycle through the connected Agent Doc tools.
```
<!-- /agent:queue -->

## Notes

<!-- agent:notes -->
<!-- /agent:notes -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#syncdoctorverify] #agent-doc-bug Confirm the #syncdoctorconverged win from /tmp/agent-doc-sync.log after several live editor tab-switch syncs on 0.35.162: doctor_repair_pre and doctor_repair_post should stay under their 250ms budget, and 'Repaired `agent-doc`/`stash` layout' should no longer appear on every sync (2764 pre + 2761 post claims with zero convergence in the pre-fix window). If either pass is still over budget, the remaining cost is repair_file_state_with_tmux's own file read, frontmatter parse, and detect_jb_cache_conflict_cancel_recoverable call rather than repair_layout.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#passthroughsplitprofile] #agent-doc-bug [blocked-on-evidence] Decide from evidence whether Claude also needs the split text+Enter submit profile that codex/opencode use (`tmux_submit_profile_for_harness`, 80ms). BLOCKED: 0.35.163 added the measurement (`route_pass_through_submit_draft ... outcome=... enters_sent=N`) minutes ago and ops.log has 0 such lines — no route dispatch has crossed the new binary yet. UNBLOCKED when ops.log holds ~20+ of those lines with harness=claude: `grep route_pass_through_submit_draft .agent-doc/logs/ops.log`. A nonzero outcome=enter_resubmit rate means the single-call send is the cause and claude should join `harness_uses_split_text_submit` (prevention); an all-cleared rate means the 2026-08-08 strand had another cause and the bounded repair is the right and only fix. Do not change the submit profile without the measurement — the mechanism is inferred, not proven.
- [/] [#stickymdpaneverify] [operator-verify] #stickymdpane live proof: restart a JetBrains IDE onto plugin 0.2.350 (Kotlin change, reload-lib is not enough), open a two-column split with an agent-doc document in each, then switch ONE column to a source file. Expect tmux to keep both panes with that column still showing its last document, instead of collapsing to one pane. Then close that split entirely and confirm tmux DOES collapse — the fallback must be memory, not invention. Unit tests pin the pure resolution but cannot prove the live IDE wiring.
- [/] [#operatorverifyresolve] [operator-verify] Resolve the current JetBrains File Cache Conflict with Cancel / Keep memory changes, then save or make one editor mutation so the plugin publishes the current editor cut; confirm the retained closeout commits and a later Compact Exchange no longer fails on the historical stranded duplicate heading.
- [/] [#operatorverifyrestart] [operator-verify] Restart a running JetBrains IDE to activate plugin 0.2.340, then move from fpe-service-topology.md in a one-pane surface to agent-doc-bugs2.md in its two-column surface and confirm the first selection restores two tmux panes without a second navigation.
- [/] [#rebootselfhealverify] [operator-verify] Live proof for #rebootselfheal after restarting the IDE onto JetBrains plugin 0.2.339 (reload-lib swaps the cdylib but NOT the Kotlin package, so a restart is required). Reboot the host (or stop the controller and leave a stale .agent-doc/controller.sock behind), then run Sync Tmux Layout and Run Agent Doc from the IDE and confirm both succeed without the 'pane-layout state projection publish failed ... Connection refused' notification and without a human deleting the socket. Then confirm the passive lane is still free: click between editor tabs with NO controller running and verify no controller is launched (editors/SPEC.md requires the surface-observation lane to never launch one) - grep ops.log for a controller bootstrap around the tab clicks. Both reboot states are already proven end-to-end against the installed cdylib; only the in-IDE path needs a human eyeball.
- [/] [#hvw0] [operator-verify] Rotate the CARGO_REGISTRY_TOKEN secret on github.com/btakita/skill-harness — the current one returns 403 authentication failed, so CI cannot publish to crates.io (v0.1.3 was published manually from the passage token btak/CARGO_REGISTRY_TOKEN). Needs a repo-secret write, which is operator-gated. Until it is rotated, every tagged release will now FAIL loudly at the publish step (that is the intended new behavior after 32836ad; it previously reported success)
- [/] [#c8tb] [operator-verify] Live #jbroutebusystall proof (0b49105e1): with the supervisor socket down (route logs `route_dispatch_only_authoritative_degraded_direct_pane supervisor_health=no_socket`) and the pane mid-turn, click JB Run Agent Doc. Expect `route_start` -> `route_success` in the attempt ledger to drop from ~17s to under ~2s, with `route_existing_pane_busy_active_turn_skip_wait` in ops.log and the SAME outcome as before (`route_existing_pane_already_running`, pane focused, no duplicate injection). Unit tests pin the cue gating but cannot prove the live latency win.
- [/] [#operatorverifylive] [operator-verify] #relaylockpoison live proof: with the JB plugin loaded, force a replica panic (the crash-2026-07-09 shape: agent_doc_replica_apply_local) and confirm later replica calls still succeed instead of returning -1/null for the life of the IDE. This is the behavior the whole migration exists to guarantee and it cannot be proven without a host IDE.
- [/] [#mtds] [operator-verify] #panedraftunblocker live proof: on a Claude pane whose footer reads `⏵⏵ bypass permissions on · 1 shell · ← for agents` (the variant without `(shift+tab to cycle)`), with an EMPTY composer and no active turn, trigger JB Run Agent Doc. Expect dispatch to succeed instead of refusing with `timed_out` / `unblocker=wait_for_dispatch_ready_prompt`. Then repeat with unsent text typed in the composer and expect the new refusal naming the draft with `unblocker=submit_or_clear_pane_draft`, and NO injection over that text. Finally confirm a genuinely mid-turn pane (live spinner, bare `❯`) still refuses as busy. Fixed in 17848a927, installed 0.35.6; needs the supervisor idle-boundary recycle to pick up the fresh binary.
- [/] [#falsetyping-guard] #agent-doc-bug provenance-aware visible-write guard. INTEGRATED to main (merge on 5bcfc07b) + binary/cdylib installed + recycled; guard tests 7/7 green; JB 0.2.205 zip + VS Code 0.2.38 compiled. REMAINING (operator): install JB plugin zip (build/distributions/agent-doc-jetbrains-0.2.205.zip) + reload IDE, install VS Code 0.2.38, then live proof — type during replica churn (no wedge) + genuine unsaved edit still fails closed. NOTE: bugs2.md can still wedge until the IDE plugin is reloaded (loaded plugin still calls FFI v2 → conservative fail-closed).
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#agentsignals] #agent-doc-feature [future/separate] Add a realtime agent:signals element after core realtime convergence is stable. Start with deterministic built-in signals for tests, queue, supervisor, and ipc; design custom user-defined signals as plugin-backed tracked work with Rust as the first target and future multi-language support; use tasks/software/realtime-signals-patchboard.md as the GUI/semi-programmatic reference; keep MLP enhancement optional/experimental and not load-bearing for deterministic states.
- [ ] [#qloop1] #agent-doc-feature Define durable queue loop annotation syntax and parser/rendering behavior: mark loop-only queue/backlog entries, keep annotation/state out of prompt completion proof, and expose a stable current-item marker for editor/supervisor surfaces.
- [ ] [#qloop2] #agent-doc-feature Implement idle-loop scheduling for annotated loop items: when normal drainable queue work is empty, dispatch loop items in deterministic sequence, persist the loop cursor/last-run state, and prove regular queue items always preempt loop work.
- [ ] [#qloop3] #agent-doc-feature Add loop poll controls: global default poll period plus per-loop override, next-run time gates, bounded backoff/jitter, and tests proving no tight spin when there are no relevant events.
- [ ] [#qloop4] #agent-doc-feature Add event-aware loop scheduling: record deterministic event counters/timestamps and let loop items schedule earlier/later based on event frequency before any model-assisted scheduling hook.
- [ ] [#qobs1] #agent-doc-feature Add a bounded observer telemetry snapshot surface: supervisor/session actors expose compact per-document state (cycle phase, queue head, actor readiness, IPC inflight, recent typed ops-log markers, and artifact handles) without dumping raw transcripts by default.
- [ ] [#qobs2] #agent-doc-feature Implement deterministic observer rules that scan session-check/preflight/ops-log snapshots for annotated issues and recurring failure patterns, then file deduped session-doc backlog bugs with evidence paths and stable issue fingerprints.
- [ ] [#qobs3] #agent-doc-feature Add a sidecar observer agent-doc loop that can monitor all running project sessions from snapshot artifacts, rate-limit bug filing, avoid pane interference, and route created bug items to the correct session document or a configured observer inbox.
- [ ] [#qobs4] #agent-doc-feature Add model-assisted observer triage as advisory-only: feed compact snapshot/context-pack handles to a subscription or local model, require deterministic evidence before auto-filing bugs, and record model provenance/confidence on suggestions.
- [ ] [#qlm1] #agent-doc-feature Define queue item backend-selection syntax for local-model execution, separate from queued `/model <tier>` switches: support explicit per-item backend/model annotations on normal and loop queue items without changing prompt text or completion proof.
- [ ] [#qlm2] #agent-doc-feature Implement a local-model queue dispatcher backend that can run eligible queue/loop items through a configured local model, capture stdout/structured results, and write responses through the normal agent-doc closeout boundary.
- [ ] [#qlm3] #agent-doc-feature Add local-model dispatch safety gates and telemetry: capability checks, GPU/resource lease integration, fallback policy, provenance logs, and tests proving local-model queue items cannot be silently marked complete without a committed response/proof.
- [ ] [#exttasksources] #agent-doc-feature Add external task source adapters that project PM-tool tasks into the active document/work graph as source-backed nodes, keeping local queue/closeout authority document-owned and making remote push-back explicit and audited. PRD: tasks/agent-doc/prd-external-task-sources.md
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
