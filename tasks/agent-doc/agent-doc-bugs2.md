---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
resume: f782d857-9380-412d-a881-13e43d75bc5b
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
queue: stop
---

## Status

<!-- agent:status patch=replace -->

completed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260808-233307.md`*

Compacted content:
- Archived 4 response topic(s): do [#ptsubmitmetric]; do [#claudesplitsubmit]; do [#compactstrandedheading]; 1 more
- Prior summary/context: prior compacted content: Archived 10 response topic(s): queuemirrororder; watchsighupwin; loopsuperprojectjob; 7 more
Compact Exchange on haiven-dev.md error"
```
agent-doc command failed (exit 1): Error: project controller command `compact_document` failed: compact: waiting for the pre-write editor delivery barrier for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/haiven-dev.md: visible document write for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/haiven-dev.md is retained by the lazy delivery projection because the editor state projection has not converged; no secondary snapshot/commit or forced disk write was attempted
```

### Re: do [#hvw0publishgrep] — opus-5

> **Queue prompt:**
>
> do [#hvw0publishgrep]


The suspicion was right, and it is now proven rather than argued.

**crates.io does not say "already exists".** Its publish controller builds the duplicate-version rejection in `duplicate_version_error`:

```rust
fn duplicate_version_error(version: &str) -> BoxedAppError {
    bad_request(format!("crate version `{version}` is already uploaded"))
}
```

So the `grep -q "already exists" publish.log` branch added in 32836ad never matched that message, and a re-run of an existing tag fell straight through to `::error::cargo publish failed`.

**But there is a second wording, and it does contain the phrase.** Modern cargo pre-checks the registry index before uploading (`verify_unpublished` in `src/ops/registry/cargo_publish.rs`) and bails with `crate <name>@<version> already exists on <registry>`. On a fresh runner with a current sparse index that check fires first, so the old grep would have matched — which is why the defect was latent rather than obviously broken. It falls through to the server message whenever the index the runner sees has not caught up, and on cargo versions predating that pre-check.

Fix: match both.

**Red first.** I moved the step into `.github/scripts/cargo-publish.sh` verbatim (old grep intact) and added `tests/release_publish.rs`, which drives it with a fake `cargo` on `CARGO`. The server-wording case failed exactly as predicted:

```
thread 'publish_tolerates_server_duplicate_version' panicked
  the remote server responded with an error (status 200 OK): crate version `0.1.3` is already uploaded
  ::error::cargo publish failed
```

**Green after widening to `grep -Eq "is already uploaded|already exists on "`.** Five cases now pinned: clean success passes through with no `::notice::`; both duplicate-version phrasings are tolerated; a `403 authentication failed` still fails the job (the regression 32836ad exists to prevent); an unrelated build failure still fails the job. The trailing space in `already exists on ` keeps the tolerance anchored to the registry sentence rather than any log line containing those words.

Your read on severity holds: the old grep could only produce a false red, never a false green, so 32836ad's guarantee was never weakened — only its documented idempotent path was broken.

**Verification:** `cargo clippy --all-features --all-targets -- -D warnings` clean; full suite 53 passed / 0 failed (exit 0, run through `rtk proxy` so the exit code is real). Committed as `081d52c` and pushed to `btakita/skill-harness` main; superproject pointer bumped in `b78c9b1`. CI run `31284439630` on that exact SHA is still in progress at closeout — recorded, not waited on.

**Not spread elsewhere.** I checked the sibling release workflows for the same pattern: `src/agent-doc` uses "already exists" for a `gh release view` check (unrelated), and `src/tsift` pre-checks the index itself via `crate_version_exists` (also unrelated). No second instance of this bug.

Still operator-gated and unchanged by this work: `#hvw0` — the `CARGO_REGISTRY_TOKEN` on that repo returns 403, so no tagged release has exercised the publish step since 32836ad. This turn proves the classifier statically; the live path stays unproven until the token is rotated and a tag runs.

### Re: do [#dedupepresettle] — opus-5

> **Queue prompt:**
>
> do [#dedupepresettle]

Fixed, and the diagnosis in the filed item was right about the shape but wrong about the count — it is **one** root cause at two sites, not three bugs.

**The root cause: a repair judged by the sample taken the instant after the write.**

`await_late_editor_repair_projection` gave its settle window only to the `LateEditorAttachment` shape and returned immediately on `Diverged`. The trap is that a projection which is *merely still propagating* also samples as `Diverged` — canonical and disk both differ from the target because neither has caught up yet — so it is indistinguishable, on that first read, from a repair that genuinely lost a race. An in-flight repair therefore got **zero** settle time and `settle_atomic_repair_projection` reported `did not converge exactly before settling deferred lineage` for a write that converged milliseconds later. The failure hashes prove it: `canonical_hash=6dec…` and `disk_hash=dd1e…` both differed from `expected_hash=1850…`, which is `Diverged`, not the late-attachment shape that had the grace path.

The window now covers every non-converged sample. The terminal classification is decided when the window closes rather than the instant after the write, so both typed refusals in `settle_atomic_repair_projection` are still reached with exactly the same semantics — only later. Renamed to `await_atomic_repair_projection` / `ATOMIC_REPAIR_PROJECTION_SETTLE_TIMEOUT_MS` since it is no longer late-editor-specific, and the wait policy moved into a pure loop over an injected sampler so it is testable without a live controller or filesystem race.

**Second site, same cause.** `agent-doc dedupe` routes through `converge_or_disk_write`, whose contract explicitly allows returning `Ok` on the deferred-delivery path — the code comment says callers needing a disk projection "separately verify `delivery_converged` at that boundary," and dedupe never did. So it printed `removed 3313 bytes` off the cut it had just handed the write path, exited 0, and left both response copies in place. Dedupe is a repair command whose entire contract is "the duplicate is gone", so it now re-reads the authority after each write, reapplies against what it actually finds (bounded at 5 attempts), and reports removed bytes only from a proven fixpoint. An unconverged repair fails loudly and names the recovery without offering the disk-write escape hatch.

**Mutation-checked, not just green.** Reverting the await policy to the old `Diverged`-returns-now behavior reddens `atomic_repair_projection_awaits_a_transiently_diverged_sample` (`left: "stale", right: "target"`); restoring it turns it green again. Four settle-policy tests plus two dedupe-convergence tests, including one proving an unconverged repair *fails* rather than reporting bytes removed.

`make check` green — 8749 tests run, 8749 passed, 0 failed. Shipped as **0.35.184**: `make release-version`, `VERSIONS.md`, `make install` (binary now reports 0.35.184), commit `c5c1b6471`, pushed, tagged `v0.35.184`, `admin recycle` issued, submodule pointer `9fe8ead`. CI / Release / PyPI runs are in progress on the tag — recorded, not awaited.

### Re: haiven-dev.md wedge and the manifest diagnosis — opus-5

Ran your three steps. The wedge is cleared, and two premises in the diagnosis need correcting.

**Steps.** `make install` is done (that build carries the fix above, so it was worth sequencing after it). `agent-doc admin reload-lib` fanned out the cdylib reload, exit 0 — but the Kotlin package still needs your IDE restart, which install itself reports (`Restart running IDEs to activate the package generation`); plugin 0.2.350 is installed in all four IDE configs. `agent-doc session doctor tasks/haiven-dev.md` now reports **`doctor: no issues detected`**. The divergence is gone.

What actually cleared it was `c0d02fdec` (`#dedupnoopretain` — session-check published a no-op repair and retained it forever), one of the two commits the running binary was missing. The other missing commit, `e6e65dd6d`, is a test.

**Correction 1 — the manifest is on disk, not live-buffer-only.** `grep -c dynamic_context_ref src/haiven-dev/tasks/haiven-dev.md` returns 2 (open and close tags). Your grep found nothing because the path is `src/haiven-dev/tasks/haiven-dev.md`; there is no `tasks/haiven-dev.md` at the superproject root. And it is not a leak — `agent-doc-compact-io` asserts `exchange.contains("<dynamic_context_ref")`, so writing the manifest reference into `exchange` is intended behavior.

**Correction 2 — it is one block with six `<context_ref/>` children, not eight directives, and the classifier does not treat it as steering.** I probed `classify_prompt_bearing_changes` and `all_unstarted_prompt_bearing_changes_from_diff` with a diff that *adds* a manifest block: both return **0** changes. So there is nothing to fix here — the "8 concurrent operator steering directives" reading was produced by the stale, wedged binary, not by a live classification defect. I did not file a backlog item for a defect that does not reproduce.

**What remains on haiven-dev.md is real steering, not a wedge.** `session-check` reports the cycle `committed` with one genuine unresolved prompt: `#pusherrorcodebranch` (push `feat/error-response-code` so the gate — "Preflight (Backend)", not "CI" — runs the `request_body_too_large` fix). Its own guidance says to continue with `agent-doc tasks/haiven-dev.md`. That document has its own owner pane (`%20`, currently `alive-busy`), so I have not driven it from this session — re-invoke it there. Doctor also flags `owned_pane_ready_busy_conflict` on that pane; it is the recoverable stale ready/busy probe, and the recovery hint is `session status` then `session clear` from another pane if it persists.

One thing your message surfaced that *is* a live defect: `agent-doc respond --help` on 0.35.184 has no `--baseline-file`, yet SKILL.md's mandatory persist step documents it. Filed as `#respondbaselineflag` at the top of the backlog.
<!-- agent:boundary:8c608826 -->
<!-- /agent:exchange -->
## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- do [#respondbaselineflag]
<!-- /agent:queue -->

## Notes

<!-- agent:notes -->
<!-- /agent:notes -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#respondbaselineflag] #agent-doc-bug SKILL.md's MANDATORY persist step documents `agent-doc respond <FILE> --baseline-file <preflight.baseline_file> --stream --origin skill`, but neither `respond` nor `write` accepts `--baseline-file` in 0.35.184 (`agent-doc respond --help` has no baseline/snapshot flag). An agent following the hot-path skill verbatim gets a clap parse error on the one step the skill calls mandatory, and the documented 'use preflight's baseline, do NOT save your own' contract has no way to be honoured. Decide one way: either add `--baseline-file` to respond/write and thread it into the diff baseline, or remove it from SKILL.md + every generated harness surface (.claude/.codex/.opencode/.cursor) and the runbooks. Reported by the operator 2026-08-08 while diagnosing the haiven-dev wedge.
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
