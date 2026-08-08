---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
resume: 49b1f99e-72a4-4dad-b63d-d5f39bd2dc8e
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260808-061455.md`*

Compacted content:
- Archived 11 response topic(s): tmux auto-sync reactive conversion; do [#opencodeharnessswitch]; tmux pane auto-sync excessive lag; 8 more
- Prior summary/context: prior compacted content: Archived 3 response topic(s): tmux auto-sync delay; do [#tmuxautosyncreactive]; do [#tmuxaut...

### Re: do [#runsubmitclaude] — opus-5

Fixed and released in **0.35.164**. The repair that was supposed to catch this
existed already — it just never looked at the pane in time to see anything.

**What actually happened.** 0.35.163 added `repair_pass_through_stranded_draft`:
after the plain-trigger single `send-keys`, capture the pane, and if the trigger
is still drafted on an idle pane, press a bare `Enter`. Your 06:15 dispatch ran
it and logged this:

```
06:15:08Z route_pass_through_submit_draft pane=%25 harness=claude
          outcome=cleared enters_sent=0 elapsed_ms=1 capture_failed=false
```

`elapsed_ms=1`. `tmux send-keys` returns as soon as the bytes reach the pty, so
one millisecond later Claude Code had not read a single keystroke, let alone
rendered one. The capture showed the pane as it was *before* the trigger
arrived — an empty composer — which `classify_pass_through_stranded_draft_action`
read as `Cleared`, a terminal verdict. Repair over, `exit_code=0`. The pane
snapshot 44 seconds later still showed
`❯ agent-doc …/agent-doc-bugs2.md` drafted on an idle pane, which is when you hit
Enter yourself.

So the check reported success on precisely the strand it exists to repair.

**Root cause.** "I have not looked yet" and "I looked and the composer is empty"
were collapsed into `!draft_visible`. That is `#idlerevisionreactive` — absence
of evidence read as evidence — applied to a pane instead of a controller probe,
and it inverts the check at the worst moment. The settle window existed but only
gated the *resubmit* side; the `Cleared` side could fire on the first frame.

**Fix.** The settle window now gates both verdicts: any unsettled observation
returns `SettleAndReobserve`, so nothing the pane shows before the window
elapses decides anything.

```rust
if !facts.settled {
    SettleAndReobserve          // <- new first arm
} else if !facts.draft_visible {
    Cleared
} else if facts.pane_busy {
    DeferredPaneBusy
} else if facts.enters_sent < facts.max_enters {
    EnterResubmit
} else {
    ExhaustedStillStranded
}
```

The common case now pays one 150ms window before returning — two orders of
magnitude under the dispatch-start proof budget the pass-through path exists to
skip, so the latency reason for that path survives intact. A genuinely stranded
trigger gets its bare submit key, up to 3 tries, and never into a busy pane.

**Coverage.** New `pass_through_repair_does_not_read_an_unrendered_pane_as_cleared`
replays your exact sequence (capture beats the render → draft appears → one
`Enter` clears it) and fails on the old ordering. The existing five replay tests
were updated for the extra leading observation; `make check` green, exit 0.

**Shipped:** 0.35.164 committed (`5bcbba512`), pushed, tagged `v0.35.164`,
`make install` done (binary + cdylib + JB 0.2.350), both controllers marked for
idle-boundary recycle. CI / Release / PyPI runs are in progress — recorded, not
awaited.

**One correction to `#passthroughsplitprofile`.** That review item was waiting on
the `enter_resubmit` rate to decide whether Claude also needs the split
text+Enter submit profile. The single measurement it got was invalid — the check
could not reach a resubmit decision at all, so `outcome=cleared` meant nothing.
As of 0.35.164 the measurement is real; the item's unblock condition is unchanged
but its evidence counter restarts from zero.
<!-- agent:boundary:9a949f6b:agent-doc-bugs2 -->
<!-- /agent:exchange -->
## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- ~~do [#passthroughsplitprofile]~~
- 🚧 do [#runsubmitclaude]
- tmux pane auto-sync is still extremely slow when switching from lazily.md to agent-doc-bugs2.md. Fix it.
- The free-text `tmux pane auto-sync is still extremely slow when switching from lazily.md to agent-doc-bugs2.md. Fix it.` was kept when you added `do [#runsubmitclaude]` into the queue. The free-text should have been replaced (deleted) by `do [#runsubmitclaude]`.
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
- [ ] 🚧 [#runsubmitclaude] Run Agent Doc does not submit to Claude Code. The prompt is written but not submitted.
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
