---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
resume: 598f30d7-3643-4e65-9e79-ec7e9d656770
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260808-080026.md`*

Compacted content:
- Archived 5 response topic(s): do [#runsubmitclaude]; closing out #runsubmitclaude; restoring the two queue items I deleted; 2 more
- Prior summary/context: prior compacted content: Archived 11 response topic(s): tmux auto-sync reactive conversion; do [#opencodeharnessswitc...

### Re: queuemirrororder — opus-5

All three parts fixed and released as **0.35.169**.

**(1) The mirror now honours the anchor.** `enqueue_actionable_ids_in_content_with_anchors` splices an anchored id directly after its anchor's live queue head instead of at the placement edge.

The anchored set is **recorded at the mutation site** (`CycleState::pending_anchored_ids`, written by the `--backlog-add-after` / `--backlog-add-before` arms in `agent-doc-write-runtime-io`), not inferred from backlog position. My first cut inferred it — "any fresh id sitting below an already-queued id is anchored" — and it reddened `closeout_sync_prepends_same_cycle_pending_add_in_go_mode`, correctly: `--backlog-add-back` produces exactly that shape and explicitly did *not* ask for the slot. Position is not evidence of intent, so the intent is recorded. Unanchored ids keep the `#queueatcreate` head default unchanged; an anchor with no live slot (struck, or completed-only) falls back to `placement` rather than vanishing. The splice stays insert-only, so operator free-text lines survive byte-for-byte.

**(2) Recovery exists.** `backlog_cmd::reorder` cascades into the mirror via `reorder_queue_mirror_in_content`, which permutes **only the named live heads among the slots they already occupy** — every other queue line keeps its exact byte offset, so `#qauthorder` holds for anything the reorder did not name. You no longer need `--done` / `--backlog-gate` to move a queued id. `queue sync` stays append-only as designed.

**(3) The metadata-only rejection is gone.** `WriteOptions::has_metadata_only_mutation` (reorder / text-metadata edits) flows into `enforce_imperative_response_contract_with_mutation_evidence`. A pure reorder has no command output, no code diff, and no commit hash to cite, so the prose heuristics rejected every honest response it could write. The binary applied the mutation, which is stronger evidence than any prose scan, so the guard defers to it.

**Changed paths:** `agent-doc-queue/src/backlog_sync.rs`, `agent-doc-element-backlog-io/src/backlog_cmd.rs`, `agent-doc-cycle-state-io/src/lib.rs`, `agent-doc-write-runtime-io/src/{lib,run_entry}.rs`, `agent-doc-write-command-io/src/lib.rs`, `agent-doc-template-io/src/{lib,write_normalize}.rs`, `agent-doc-preflight-io/src/lib.rs`, `agent-doc-start-runtime-io/src/lib.rs`, `tests/pending_integration.rs`, `SKILL.md`, `runbooks/pending-ops.md`, `VERSIONS.md`.

**Coverage added:** anchor honoured over placement; two ids sharing an anchor keep backlog order behind it; unanchored id below a queued one still takes the head; non-live anchor falls back; reorder permutes only named heads (free-text line asserted at a fixed slot); reorder is a no-op when already ordered; end-to-end `closeout_sync_places_anchored_same_cycle_add_after_its_anchor`; CLI-level `backlog_reorder_cascades_to_queue_mirror`; guard defers to the mutation but still rejects the same prose without it.

**Verification:** `make check` — 8707 passed, 0 failed, 248 skipped. `make install` + `agent-doc admin recycle` (binary now reports 0.35.169). Committed `eefc45681`, pushed, tagged `v0.35.169`.

**CI:** run `31248836014` was still queued at closeout — not awaited per `#ci-no-closeout-wait`.

### Re: watchsighupwin — opus-5

Verified. The Windows wheel built and shipped for 0.35.167 — nothing to fix.

**The gate is real and correct.** `agent-doc-watch-io/src/daemon.rs:347-353` — `install_shutdown_signal_handlers()` has a `#[cfg(unix)]` body installing `SIGTERM`/`SIGINT`/`SIGHUP` handlers and a `#[cfg(not(unix))]` no-op, because `libc::SIGHUP` does not exist on `*-pc-windows-msvc`.

**Artifact proof, not a green job.** A green publish leg proves nothing on its own, so I checked the published artifact:

- PyPI run `31246515464` (tag `v0.35.167`) succeeded, and `build (x86_64-pc-windows-msvc, windows-latest)` is `success` in its job matrix.
- PyPI serves `agent_doc-0.35.167-py3-none-win_amd64.whl` (17,446,595 B) alongside the three unix wheels.
- Downloaded it: it contains `agent_doc-0.35.167.data/scripts/agent-doc.exe`, 47,143,424 B, header `MZ` / `PE\0\0`, COFF machine `0x8664` (`IMAGE_FILE_MACHINE_AMD64`). So the cfg-gated code compiled *and linked* into a real AMD64 Windows binary — an empty or stub wheel would not.
- No regression since: 0.35.168 also publishes `win_amd64`. 0.35.169's PyPI run (`31248835628`) is still in progress, not awaited (`#ci-no-closeout-wait`).

**Separate finding, filed not fixed.** While checking runs I found the **agent-loop superproject** `Agent Doc` workflow's `check` job red for 8 consecutive runs: `collect2: fatal error: ld terminated with signal 7 [Bus error], core dumped` linking `agent-doc-commit-io` test `"git"` and `agent-doc-stream-io` lib test. `tmux-ci` passes in the same runs and local `make check` is green (8707/8707), so this is the runner exhausting memory/disk linking a 144-crate workspace's test targets — a CI-infra class, not a code regression, and a different repo's workflow from the one this head asked about. Filed at the top of `agent:backlog` rather than landing a speculative runner-tuning change mid-drain.

Session note: exchange accretion is at `warn` (2 no-op closeouts in 30 min); queue is active so I'm not stalling to compact.

### Re: loopsuperprojectjob — opus-5

> **Queue prompt:**
>
> do [#loopsuperprojectjob]


Root-caused and fixed. It was never a code regression.

**Diagnosis.** `collect2: fatal error: ld terminated with signal 7 [Bus error], core dumped` linking `agent-doc-commit-io` test `"git"` and `agent-doc-stream-io` lib test. SIGBUS from a linker is the ENOSPC signature: `ld` mmaps its output file, and a write into a mapping that can't be backed raises SIGBUS rather than returning an error, so a full disk surfaces as a core dump instead of "no space left on device". Building the ~156-crate workspace with `--all-targets` while Cargo retains incremental state and full debug info for every test binary exceeds the ~14 GB free on `ubuntu-latest`.

**The controlled comparison made it conclusive.** agent-doc's own `.github/workflows/ci.yml` runs the *same* `make check` on the *same* runner image and has never produced this failure — because it already carries the mitigation, with a comment naming this exact cause:

```yaml
CARGO_INCREMENTAL: "0"
CARGO_PROFILE_DEV_DEBUG: "0"
CARGO_PROFILE_TEST_DEBUG: "0"
```

The superproject workflow simply never inherited it. (The one red run in agent-doc's own CI, `31245808892`, was a transient `install-action` failure — a different signature.)

**Fix** — `.github/workflows/agent-doc.yml`, commit `8b01b6fe50`:
- All three env vars at **workflow scope**, so `tmux-ci` gets them too. It builds the same `--all-targets` graph and is sitting on the same cliff; it just hasn't tipped yet.
- `cargo-nextest` install on `check`. Without it `make test` silently falls back to plain `cargo test`, so the superproject was running with **no per-test deadlock guard** — `.config/nextest.toml`'s 60s x 3 timeout was inactive there. A lock/IPC deadlock would have hung to the job cap instead of failing fast with a named test.
- `timeout-minutes: 60` on both jobs as the backstop for non-test hangs.
- A comment tying the two workflows together, since a divergence here reads as a code regression.

**Verification:** `make check` with all three env vars set locally — **8707 passed, 0 failed, 248 skipped**. That proves the flags don't red the suite; it can't prove the disk headroom, which is runner-specific. Pushed; run `31249383368` is in progress, observed once and not awaited (`#ci-no-closeout-wait`). Filed an actionable follow-up to confirm it went green, with the next escalation levers if the reduction turns out to be insufficient.

Session note: exchange accretion is `warn` (6 cycles / 30 min); queue is active so I'm not stalling to compact.
<!-- agent:boundary:9a949f6b:agent-doc-bugs2 -->
<!-- /agent:exchange -->
## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- do [#confirmsuperprojectjob]
- ~~do [#loopsuperprojectjob]~~
- ~~do [#queuemirrororder]~~
- ~~do [#watchsighupwin]~~
- do [#tmuxsyncslow]
- do [#runpromptverbose]
<!-- /agent:queue -->

## Notes

<!-- agent:notes -->
<!-- /agent:notes -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#confirmsuperprojectjob] Confirm the superproject `Agent Doc` / `check` job went green after the runner-disk fix (commit 8b01b6fe50, run 31249383368): `gh run list --workflow "Agent Doc" --limit 2`. If the Bus error recurs, the disk reduction was insufficient — next levers are freeing runner disk (drop the preinstalled Android SDK / dotnet / GHC images), `-C debuginfo=0` via RUSTFLAGS, or splitting `--all-targets` across jobs.
- [ ] [#tmuxsyncslow] tmux pane auto-sync is still extremely slow when switching from lazily.md to agent-doc-bugs2.md. Fix it. (restored from the queue after #qftnodoneeat deleted it; plan context: tasks/agent-doc/plan-tmux-auto-sync-reactive.md)
- [ ] [#runpromptverbose] The prompt Run Agent Doc sends to Claude Code should simply be `/agent-doc .../file.md`. It currently sends a long multi-paragraph prompt ("Agent Doc queue state advanced for ... Continue this existing owner-pane session; do not invoke agent-doc recursively. Execute the current active queue head: ... Persist and finalize this cycle through the connected Agent Doc tools."). Restored from the queue after #qftnodoneeat deleted it.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#passthroughsplitprofile] #agent-doc-bug [blocked-on-evidence] Decide from evidence whether Claude also needs the split text+Enter submit profile that codex/opencode use (`tmux_submit_profile_for_harness`, 80ms). BLOCKED: the only measurement so far was invalid — 0.35.163's draft check classified an unrendered pane as `outcome=cleared enters_sent=0 elapsed_ms=1` and could never reach a resubmit decision, so its verdict carried no information. 0.35.164 (#runsubmitclaude) gates that verdict on the settle window, so the counter is real from now and restarts at zero. UNBLOCKED when ops.log holds ~20+ post-0.35.164 lines with harness=claude: `grep route_pass_through_submit_draft .agent-doc/logs/ops.log`. A nonzero outcome=enter_resubmit rate means the single-call send is the cause and claude should join `harness_uses_split_text_submit` (prevention on top of the repair); an all-cleared rate means the settle-gated repair is sufficient and the submit profile should not change. Do not change the submit profile without the measurement — the mechanism is inferred, not proven.
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
