---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
resume: fc9b6f02-d703-42c6-97ab-8f1d5e6ea4a2
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260808-173725.md`*

Compacted content:
- Archived 10 response topic(s): queuemirrororder; watchsighupwin; loopsuperprojectjob; 7 more
- Prior summary/context: prior compacted content: Archived 5 response topic(s): do [#runsubmitclaude]; closing out #runsubmitclaude; restoring...

*5 earlier topic(s) archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260808-214213.md`*

### Re: do [#ptsubmitmetric] — opus-5

Fixed and shipped in **0.35.181**. The criterion was worse than imprecise — it counted an event that cannot exist.

**Why it was structurally zero.** `EnterResubmit` is a *non-terminal* action. The repair loop `continue`s on it and only terminal actions reach `log_op`, so `outcome=enter_resubmit` can never appear in a production ops.log. "Unblock when the `outcome=enter_resubmit` rate is nonzero" could not have been satisfied by any amount of real-world breakage.

**The signal that does exist.** `enters_sent` is incremented only inside the `EnterResubmit` branch, so a nonzero count on a *terminal* line means the initial single-call text+Enter send did not submit. Reading that still meant hand-parsing every line and knowing the above, so the terminal line now states the conclusion:

```
route_pass_through_submit_draft ... outcome=cleared enters_sent=1 resubmit_required=true ...
```

Note the outcome is `cleared` in both the clean and the repaired case — `resubmit_required` is the only thing that separates them, which is exactly why hand-parsing was error-prone.

**Rollup: `agent-doc ops submit-profile [--since <RFC3339 prefix>] [--json]`.** Per harness: repairs, how many needed a bare submit key, the rate, the outcome breakdown, `max_enters_sent`. Run against this project's live log just now:

```
claude: 11 repair(s), 8 needed a bare submit key (72%), max enters_sent=2
    outcome=cleared: 11
  note: 11 line(s) predate `resubmit_required` and were counted from `enters_sent`
```

Legacy lines are recovered from `enters_sent` but reported separately, so a mixed sample is declared rather than quietly folded in.

**Restated criterion for `#passthroughsplitprofile` / `#claudesplitsubmit`:** unblock when `agent-doc ops submit-profile --since <first post-0.35.164 day>` reports **≥20 `harness=claude` repairs**; the call is then `resubmit_required` / `repairs`. Current reading is **11 repairs, 8 resubmit-required (72%)** — up from the 6 samples the review item recorded, still under the floor. The rate is high enough to be suggestive, but 11 samples is not the agreed floor, so `#claudesplitsubmit` stays measurement-blocked rather than decided. It is now one command to re-check instead of a grep session.

Tests pin that the two non-terminal actions never reach the log (the reason the old counter was zero), that a repaired and a clean send both report `outcome=cleared` but differ on `resubmit_required`, and both rollup paths including the legacy recovery and `--since` filtering.

**Shipped:** `make check` green (exit 0 — clippy caught a manual checked division on the first pass and it was fixed), `make install` → 0.35.181, pushed (`c62d36acd` + superproject `40801afe96`), controllers auto-recycled by `lib-install`. CI runs `31277544182` and `31278434421` still in progress at closeout — recorded, not awaited.

### Re: do [#claudesplitsubmit] — opus-5

> **Queue prompt:**
>
> do [#claudesplitsubmit]


Decided on measurement: **claude stays on the single-call submit.** No profile change. Shipped as **0.35.182**, which records the evidence rather than changing behavior.

The item was blocked on reaching ~20 samples. Rather than leave it accruing, I gathered the rest under control with the metric that shipped an hour ago.

**12 controlled dispatches into an idle, unloaded pane (0.35.182, 50-row): 12/12 submitted on the first send, 0 resubmits.**

I did not trust that number on its own. Every line reported `elapsed_ms=305` — exactly the minimum two-settle path — which is also precisely the shape a *false* clear takes if the keystrokes have not rendered by the second look. That is the `#runsubmitclaude` ambiguity, so `outcome=cleared` is not self-validating. Pane scrollback cannot settle it either: Claude's TUI redraws in place, so `history_size` is **8 lines** and holds no transcript at all — counting trigger echoes there gave 6, which is just what happened to be on screen.

The authoritative record is Claude's own session JSONL, and it has **exactly 12 submitted trigger prompts for 12 dispatches**, one per dispatch, 85 s apart. The 0% is real.

**Against 11 production repairs from the same day under real load: 8 needed a bare submit key (72%), `max_enters_sent=2`.**

**So the split is the interesting result, not the average.** The drop is *load-dependent*, not inherent to the single-call send — on a quiet pane the send never fails. And across all 23 samples every single line is `outcome=cleared` with **zero `exhausted_still_stranded`**: the settle-gated repair recovered every strand. No dispatch has ever been lost.

Adding claude to the split profile would pay 80 ms on every submit to remove a retry that already succeeds 100% of the time. So the answer is the second branch the item allowed: **the settle-gated repair alone is sufficient.**

Worth being explicit about the limits: 12 of the 23 samples are mine, sequential, on an idle scratch pane, and the 11 production ones are legacy lines counted from `enters_sent`. If the resubmit latency under load ever matters on its own, that is a separate optimization with its own measurement — and `agent-doc ops submit-profile --since <day>` re-checks it in one command instead of a grep session.

The reasoning now lives on `harness_uses_split_text_submit` and in a test named for the decision, so a future change has to confront the measurement rather than re-derive the inference. `#passthroughsplitprofile` is resolved by this.

**Shipped:** `make check` green (exit 0, 0 failed suites), `make install` → 0.35.182, pushed (`3b6ff4287` + superproject `ca42452ac3`). Scratch repro project and probe tmux session cleaned up.

### Re: do [#compactstrandedheading] — opus-5

> **Queue prompt:**
>
> do [#compactstrandedheading]


Confirmed: `compact exchange` no longer fails on the stranded duplicate heading. No code change was needed — but the obvious way to check this gives a **false pass**, so the interesting part is how it was verified.

**The vacuous check.** Running compact on this document worked (281 → 163 lines, 5 topics archived, queue and backlog untouched). That proves nothing: the document has **no duplicate headings** — 7 distinct `### Re:` topics, every one unique. An earlier compaction had already carried the historical shape away. A green run over input that lacks the defect is exactly the vacuity trap.

**The real check.** I built the shape directly — an exchange with a body-bearing `### Re: first topic`, a second topic, an **empty** `### Re: first topic` shell, and a third topic — and compacted that:

- `rc=0`, no failure
- the empty duplicate shell removed, `first topic` now appears **zero** times in the document
- the real bodies of the kept topics intact
- the displaced topic archived **with its body**, not dropped
- `agent:backlog` `#keepme` untouched
- `--keep 2` counted topics *after* dedup: 4 headings → 3 real topics → archive 1, keep 2

And the proof the repair path actually ran rather than the input being harmless: `compact_repaired_stranded_duplicate_response_heading` in ops.log, stamped at exactly the fixture's compact time.

**Pinned.** The repair function itself was already covered in `agent-doc-turn`. What can silently regress is the **wiring** — compact dropping the call, or growing its own private normalization — so the new test in `agent-doc-compact-io` pins that plus the ops marker, alongside the behavioural assertion that the shell goes and both bodies stay.

`make check` green (exit 0, 0 failed suites). Pushed (`e6e65dd6d` + superproject `4dc62ed3ed`). No version bump: this is test-only with no behaviour delta.

One note on this document: the compact above was a real mutation — the exchange is now a session summary plus the two most recent topics, with 5 topics archived to `.agent-doc/archives/…-20260808-214213.md`.
<!-- agent:boundary:22e8b690:agent-doc-bugs2 -->
<!-- /agent:exchange -->
## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- ~~do [#clearsubmitlabel]~~
- ~~do [#startupmiss9row]~~
- ~~do [#ptsubmitmetric]~~
- ~~do [#claudesplitsubmit]~~
- ~~do [#compactstrandedheading]~~
- do [#hvw0publishproof]
<!-- /agent:queue -->

## Notes

<!-- agent:notes -->
<!-- /agent:notes -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#hvw0publishproof] #agent-doc-bug Verify statically (read the release workflow on github.com/btakita/skill-harness) that the crates.io publish step propagates failure after 32836ad. The loud-failure behavior has never actually executed: the newest Release run is v0.1.3 from 2026-07-18 and its `Publish to crates.io` step reports SUCCESS even though the CARGO_REGISTRY_TOKEN 403s and v0.1.3 was published by hand — that success line is the false green 32836ad was meant to kill. Until a tag runs, the every-tagged-release-now-fails-loudly claim on #hvw0 is untested.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#stickymdpaneverify] [operator-verify] #stickymdpane live proof: restart a JetBrains IDE — plugin 0.2.350 is ALREADY INSTALLED in all four IDE configs, so only the restart is missing, and that one restart also satisfies the install precondition for #operatorverifyrestart, #rebootselfhealverify and #falsetyping-guard — then open a two-column split with an agent-doc document in each and switch ONE column to a source file. Expect tmux to keep both panes with that column still showing its last document, instead of collapsing to one pane. Then close that split entirely and confirm tmux DOES collapse — the fallback must be memory, not invention. Unit tests pin the pure resolution but cannot prove the live IDE wiring.
- [/] [#operatorverifyrestart] [operator-verify] Restart a running JetBrains IDE (installed plugin is 0.2.350; this item originally named 0.2.340, long superseded — the single restart in #stickymdpaneverify covers it), then move from fpe-service-topology.md in a one-pane surface to agent-doc-bugs2.md in its two-column surface and confirm the first selection restores two tmux panes without a second navigation.
- [/] [#rebootselfhealverify] [operator-verify] Live proof for #rebootselfheal after restarting the IDE (installed plugin is 0.2.350; this item originally named 0.2.339 — reload-lib swaps the cdylib but NOT the Kotlin package, so a restart is still required, and the restart in #stickymdpaneverify covers it). Reboot the host (or stop the controller and leave a stale .agent-doc/controller.sock behind), then run Sync Tmux Layout and Run Agent Doc from the IDE and confirm both succeed without the `pane-layout state projection publish failed ... Connection refused` notification and without a human deleting the socket. Then confirm the passive lane is still free: click between editor tabs with NO controller running and verify no controller is launched (editors/SPEC.md requires the surface-observation lane to never launch one) - grep ops.log for a controller bootstrap around the tab clicks. Both reboot states are already proven end-to-end against the installed cdylib; only the in-IDE path needs a human eyeball.
- [/] [#hvw0] [operator-verify] Rotate the CARGO_REGISTRY_TOKEN secret on github.com/btakita/skill-harness — the current one returns 403 authentication failed, so CI cannot publish to crates.io (v0.1.3 was published manually from the passage token btak/CARGO_REGISTRY_TOKEN). Needs a repo-secret write, which is operator-gated. CORRECTION 2026-08-08: the claim that every tagged release now FAILS loudly at the publish step (intended behavior after 32836ad) is UNTESTED — no tag has run since. The newest Release run is v0.1.3 from 2026-07-18 and its `Publish to crates.io` step reports success, which is exactly the false green. #hvw0publishproof carries the static check.
- [/] [#c8tb] [operator-verify] Live #jbroutebusystall proof (0b49105e1): with the supervisor socket down (route logs `route_dispatch_only_authoritative_degraded_direct_pane supervisor_health=no_socket`) and the pane mid-turn, click JB Run Agent Doc. Expect `route_start` -> `route_success` in the attempt ledger to drop from ~17s to under ~2s, with `route_existing_pane_busy_active_turn_skip_wait` in ops.log and the SAME outcome as before (`route_existing_pane_already_running`, pane focused, no duplicate injection). Unit tests pin the cue gating but cannot prove the live latency win.
- [/] [#operatorverifylive] [operator-verify] #relaylockpoison live proof: with the JB plugin loaded, force a replica panic (the crash-2026-07-09 shape: agent_doc_replica_apply_local) and confirm later replica calls still succeed instead of returning -1/null for the life of the IDE. This is the behavior the whole migration exists to guarantee and it cannot be proven without a host IDE.
- [/] [#mtds] [operator-verify] #panedraftunblocker live proof: on a Claude pane whose footer reads `⏵⏵ bypass permissions on · 1 shell · ← for agents` (the variant without `(shift+tab to cycle)`), with an EMPTY composer and no active turn, trigger JB Run Agent Doc. Expect dispatch to succeed instead of refusing with `timed_out` / `unblocker=wait_for_dispatch_ready_prompt`. Then repeat with unsent text typed in the composer and expect the new refusal naming the draft with `unblocker=submit_or_clear_pane_draft`, and NO injection over that text. Finally confirm a genuinely mid-turn pane (live spinner, bare prompt) still refuses as busy. Fixed in 17848a927. This item named binary 0.35.6 and a pending idle-boundary recycle; the installed binary is now 0.35.174 and has recycled many times, so only the live eyeball remains.
- [/] [#falsetyping-guard] #agent-doc-bug provenance-aware visible-write guard. INTEGRATED to main (merge on 5bcfc07b) + binary/cdylib installed + recycled; guard tests 7/7 green. JB install step DONE — the installed plugin is 0.2.350 in all four IDE configs, superseding the 0.2.205 zip this item named; only an IDE restart is missing, covered by #stickymdpaneverify. VS Code 0.2.38 install is UNVERIFIED — no agent-doc extension found under ~/.vscode/extensions. REMAINING (operator): restart the IDE, then live proof — type during replica churn (no wedge) + genuine unsaved edit still fails closed.
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
