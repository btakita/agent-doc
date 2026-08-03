---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
resume: ee757fa4-31c3-4c1b-9943-1b004f037a8a
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

complete
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260802-234338.md`*

Compacted content:
- Archived 8 response topic(s): Review triage; Closeout convergence, live-session recovery, and “legacy Markdown”; `#f1s3`; 5 more
- Prior summary/context: prior compacted content: Archived 2 response topic(s): Response; haiven-dev retained closeout and editor route

### Re: #realtimesalient — gpt-5

The real-time salient-response path was already implemented and released in agent-doc `0.35.117`; the installed build is `0.35.118`. I verified the structural salient checkpoint, CRDT/Lazily response node, final-response replay/cleanup, specification, runbook, release note, and focused Rust coverage. The queue item is complete.

### Re: tmux auto-sync latency — gpt-5

The manual `Sync Tmux Layout` run completed in about four seconds; its largest measured costs were ownership proof/per-file ownership work and tmux routing. The later editor tab switch was a separate failure: it never published an editor-surface observation to the controller, so the apparent wedge was before tmux reconciliation.

The cause was JetBrains selection ordering under load. `selectionChanged` could arrive while IDEA still exposed the old file through `selectedFiles`; after three immediate EDT reprojection passes, the listener retained the observation but had no dependency edge that would wake it again. The listener now carries the selection event's old file and, only after the bounded settling budget is exhausted, substitutes the exact old-to-new event edge into a still-stale visible/layout projection. It never invents a pane or replacement unless the old event file is actually present. The JetBrains specification records this behavior.

Regression coverage first failed on the missing event-edge reconciler, then passed after implementation. The focused `EditorTabSyncListenerTest` and the repository-wide `make check` both pass. The CLI/native library and JetBrains plugin package were installed locally; two live native endpoints reloaded and one was unavailable, while the Kotlin listener package will activate on the next IDE restart. The implementation is committed as `d471faf5d` (`fix: settle stale editor selection observations`).

The previously red hosted CI run was unrelated to this listener: Gradle could not download unpublished `io.github.lazily:lazily:0.39.0`. CI now checks out the public `lazily-kt` sibling composite dependency at the path already used by local builds, and the full local suite passes through that path.

### Re: haiven Compact Exchange commit — gpt-5

> **Queue prompt:**
>
> do [#realtimesalient]


The earlier haiven session-check proved a synthetic closeout but did not prove the response repository commit. I corrected that: `tasks/haiven-dev.md` and its done archive are committed in the haiven-dev repository as `f9a49e4`. `agent-doc session status` now reports `committed_after_latest_run=true`, and session doctor reports no issues. A newer four-line queue/in-progress editor projection remains outside that response commit so concurrent document state was not swept into it.

### Re: Response

Implemented and committed `#compacttombgc` in agent-doc source as `1ffbc7b98` (`fix: fence compact epochs and reload stale admission hooks`). Compact Exchange now crosses a retained CRDT epoch fence only after its compacted snapshot has converged and optional commit closeout has consumed the old delivery frontier. An immediately stable relay rebuilds the canonical from one snapshot insertion, rotates lineage, discards accumulated insert/delete history, quarantines same-text stale deltas, and queues every live replica for replace-capable re-bootstrap. If a newer post-commit delivery is still in flight, the compaction request persists in the retained canonical projection and settles when the final exact visible projection arrives. Detached and explicit force-disk paths do not invent replica stability.

Added regressions proving same-text history shrink plus stale-lineage fencing, and retained settlement on the final peer projection. Updated the realtime and compact-exchange contracts.

Also fixed the reported stale Claude admission-hook snapshot path while this source build was open: prompt-bearing dispatch-only tmux delivery now requires the same per-document cycle acknowledgment as managed delivery. Accepted pane input alone is no longer success. A missing acknowledgment triggers one fresh supervisor restart for Claude Code or Codex, reloading startup hook settings, then retries and fails closed if admission is still absent. Updated the session/harness contracts and added supported-harness recovery coverage.

Verification: focused realtime/relay/compact suites passed (384 tests); route-io passed (72 tests, 13 live-tmux tests ignored); `make check` passed all 8,553 tests with 244 skipped, release-projection checks, and all TLA+ models. Changed public files passed the private-name scan and `git diff --check`. Installed the exact committed binary/library and refreshed skills/editor packages.

### Re: do [#reactive-closeout-install-verify] — codex

Installed and exercised the committed reactive-projection build. The retained closeout crossed a terminal boundary cleanly, and all acceptance checks were performed without recycling an active closeout owner.

- **Installed source:** Agent Doc commit `70a407d15` (`refactor: project turn state reactively`) is pushed on `origin/main`; `make install` installed Agent Doc `0.35.118`, the native library, skills, and JetBrains plugin package `0.2.338`.
- **tsift.md terminal delivery:** the fresh installed build admitted and finalized `cycle-1785724031373`; the response committed as superproject commit `16ca39dfc`. The first template-less write failed before mutation, and the correctly wrapped retry reached committed terminal state without a follow-up session check.
- **Equity Funding Source queue strike:** read-only inspection found committed response `### Re: do [#recommendedprunestale]` and commit `8884a04d8`; the queue has advanced to active head `#recommendeddeploybrowser`. Its live owner remained untouched—no admission, recycle, or closeout interference.
- **Monster Rod Holders corruption rejection:** the wrong-pane direct write was rejected before mutation by the pane-ownership guard. The canonical owner then restored the missing capture and committed `947cfff4`; `#accessoriesspendredirect` exists once, while `#ga4-collection-gap-724` remains unconsumed. The already-applied production state was not replayed; a read-only dry run reported `needs_apply=false changed=0` for all four product IDs.
- **Compact Exchange recovery:** `#compacttombgc` finalized successfully in `cycle-1785716963463`; the compact epoch/reload fence is present in commit `1ffbc7b98`, and the queue continued normally afterward.
- **Responsiveness:** tsift editor delivery admitted and committed through the reactive state stream, the Monster Rod Holders owner-pane repair committed normally, and the active Equity Funding Source and Lazily owners were observed without recycle/reload or duplicate response submission.

**Changed paths:** the implementation is concentrated in `agent-doc-controller-io/src/project_controller/rpc.rs` plus controller/supervisor/editor adapters, tests, and `specs/14-realtime-workflow.md`; unary turn-request/ACK paths were removed in favor of typed `TurnAdmissionProjectionOperation::{Current, Await { baseline_cycle_id, wait_ms }}` state projection.

**Commands/proof:** `make check`, workspace `cargo check`, focused Rust tests, VS Code tests (169/169), JetBrains Gradle tests, `make install`, live Agent Doc admit/finalize cycles, `tsift status` + indexed source inspection, Git history inspection, and the Monster Rod Holders exclusion dry run. Local checks are green. GitHub CI run `30779663452` for `70a407d15` is still in progress with no reported failure, so it was observed but not awaited.

**Confidence:** high. **Escalation:** none.

### Re: do [#multilineheadbullet] — gpt-5

> **Queue prompt:**
>
> do [#multilineheadbullet]


Implemented and installed the multiline queue-head addressability fix.

The invariant is now explicit: queue parsing, AST node enumeration, and queue completion operate on the same ordered logical item sequence. `agent-doc-markdown-ast::overlay::item_nodes` owns addressability policy through the typed `ItemSurface::{ListItem, MultilinePrompt}` boundary. Canonical bare `---` and `~~~prompt` blocks enumerate as multiline prompt nodes; completed `~~~done` blocks preserve the same logical node identity. This is a pure, one-shot document projection, so there is no long-lived reactive relationship to extract into `Computed` state.

Transition coverage now proves:

- live bare/`~~~prompt` multiline head → addressable AST node → `~~~done` completion;
- completed multiline node → stable node key → unstrike restores the live bare form;
- malformed or otherwise unaddressable input → fail closed without striking the neighbouring list item.

The edited surfaces are sufficient because the Markdown AST crate owns node surfaces and mutations, queue I/O supplies the end-to-end consume regression, queue crates document the shared ordering/guard contract, and the document-format specification records the canonical behavior.

Proof:

- Source commit: `2fa106c6a fix: address multiline queue heads by node`, pushed to `origin/main`.
- `cargo test -p agent-doc-markdown-ast multiline_queue_head_is_addressable_and_consumes_without_touching_neighbour` — green after first confirming the focused test failed before implementation.
- `cargo test -p agent-doc-queue-io multiline_head_is_addressable_and_strikes_without_touching_a_neighbour` — green.
- `cargo test -p agent-doc-markdown-ast` — green.
- `cargo test -p agent-doc-queue -p agent-doc-queue-io` — green.
- `make check` — green (`.tsift/artifacts/tart-0097c36ae7.test.log`).
- `cargo fmt --all` and `git diff --check` — clean.
- `make install` — installed Agent Doc `0.35.118`, native library, skills, and JetBrains plugin `0.2.338`.
- Latest GitHub CI run `30780651967` for `2fa106c6a` was still in progress at closeout.

Confidence: high. Escalation: none.





















### Re: do [#multilineheadbullet] — gpt-5

Implemented and installed the multiline queue-head addressability fix.

The invariant is now explicit: queue parsing, AST node enumeration, and queue completion operate on the same ordered logical item sequence. `agent-doc-markdown-ast::overlay::item_nodes` owns addressability policy through the typed `ItemSurface::{ListItem, MultilinePrompt}` boundary. Canonical bare `---` and `~~~prompt` blocks enumerate as multiline prompt nodes; completed `~~~done` blocks preserve the same logical node identity. This is a pure, one-shot document projection, so there is no long-lived reactive relationship to extract into `Computed` state.

Transition coverage now proves:

- live bare/`~~~prompt` multiline head → addressable AST node → `~~~done` completion;
- completed multiline node → stable node key → unstrike restores the live bare form;
- malformed or otherwise unaddressable input → fail closed without striking the neighbouring list item.

The edited surfaces are sufficient because the Markdown AST crate owns node surfaces and mutations, queue I/O supplies the end-to-end consume regression, queue crates document the shared ordering/guard contract, and the document-format specification records the canonical behavior.

Proof:

- Source commit: `2fa106c6a fix: address multiline queue heads by node`, pushed to `origin/main`.
- `cargo test -p agent-doc-markdown-ast multiline_queue_head_is_addressable_and_consumes_without_touching_neighbour` — green after first confirming the focused test failed before implementation.
- `cargo test -p agent-doc-queue-io multiline_head_is_addressable_and_strikes_without_touching_a_neighbour` — green.
- `cargo test -p agent-doc-markdown-ast` — green.
- `cargo test -p agent-doc-queue -p agent-doc-queue-io` — green.
- `make check` — green (`.tsift/artifacts/tart-0097c36ae7.test.log`).
- `cargo fmt --all` and `git diff --check` — clean.
- `make install` — installed Agent Doc `0.35.118`, native library, skills, and JetBrains plugin `0.2.338`.
- Latest GitHub CI run `30780651967` for `2fa106c6a` was still in progress at closeout.

Confidence: high. Escalation: none.

### Re: #advance-review — gpt-5

Reviewed the 16 remaining gated items against the current session state and agent-doc `0.35.118` / JetBrains plugin `0.2.338`. None has new document evidence sufficient to resolve it: each still names live IDE/pane observation, operator-secret access, or another explicit external proof.

Added one canonical recommended follow-up for every review item, in existing review priority order. The document’s priority-queue policy mirrors these backlog additions into the queue, so subsequent cycles can validate each gate, refresh stale version-specific instructions where necessary, and then either resolve it or leave a precise operator blocker:

`#compactreactiveprojectionverify`, `#steeringmirrorverifylive313`, `#preflightinbinary`, `#jbsurfaceswap`, `#boundaryaccumverify`, `#r4gq`, `#zqv8`, `#compactindcellsverify`, `#crdtpushdrainverify`, `#bbpe`, `#hvw0`, `#c8tb`, `#operatorverifylive`, `#mtds`, `#editormodelmissing`, and `#falsetyping-guard`.

Changed paths: only this session document through binary-owned granular backlog mutations. Commands/evidence: admitted and planned the connected cycle, read the document, ran `tsift status`, queried `agent-doc review list --json`, inspected the canonical `review ungate-tasks` contract, and checked the current release notes. Verification: 16 gated review items produced 16 ordered recommended follow-ups; no source or test files changed. Confidence: high. Escalation: none.

### Re: Response

Processed `#advance-review` and committed the cycle.

- Reviewed all 16 gated items; none had sufficient evidence to close.
- Added 16 ordered recommended validation/ungate tasks to the backlog and queue.
- Closeout reached terminal `commit_success`.
- No code or test files changed.

### Re: do [#percellconverge] — opus-5

Shipped the write-path half in `0.35.124`: one ownership predicate, called from all three refusal sites.

The plan's diagnosis was right, and the irony it recorded is the whole lesson. `crdt_relay_pending_refusal`'s doc comment says it was made a single function "so all three call sites give the same instruction rather than drifting into three dialects." Consolidating the *wording* did not stop the *predicate* from diverging — each site went on asserting the deferral unconditionally, and `0.35.123` fixed only one of them.

The predicate now lives once, in `agent_doc_turn::write_ownership`: `RetainedWriteOwnership { cycle_open, retained_capture }` → `verdict()` → `retained_write_remedy()`. `agent_doc_capture_io::retained_write_ownership` is the I/O shell that reads the two durable facts. All three sites derive their wording from it, so `write --commit` can no longer print "a deferral, not a lost response" while `session-check` calls the same state stranded.

Three decisions worth stating, because each is the direction that loses work if taken wrong:

- **A failed read is not ownership.** An unproven site reports stranded and names the recovery. Assuming a holder is the failure being removed.
- **The 2026-07-26 guidance is unchanged for the owned case.** "Do NOT re-send / force disk / `admin recycle` / `admin reload-lib`" was written for a real incident where two sessions invented recoveries that each perturbed the capture being awaited. Only the *unowned* case gains a remedy.
- **Ownership is not keyed on editor attachment.** `TransportProjection::editor_generation` is a one-way latch (`#editormodelmissing`); a predicate that trusted it would answer "owned" for every document ever opened in an editor.

Two guards in `agent-doc-turn/tests/retained_write_ownership_guard.rs` hold both halves — no site may re-author a verdict phrase, and every site must actually call the predicate. Worth recording that the second guard's first draft was **satisfied by a doc comment** that merely mentioned the module: deleting the call kept it green. It now matches call forms on non-comment lines only. A guard satisfied by prose about the rule is not a guard.

Mutation-checked red three ways: re-authoring a verdict phrase in `session-check`, deleting the predicate call from `document-realtime-io`, and forcing `verdict()` to always return `Deferred`.

The four per-component phases are not done. They are now separate backlog items rather than a parent that gets gated after partial progress, and the plan file records the write-path half as shipped.

**Proof.** `4abb52061`. `make check` — 8558 passed, 244 skipped, all TLA+ models green. Installed `0.35.125`. Not pushed — you declined that step.

### Re: parallel haiven-dev.md sessions — opus-5

Found concrete evidence in `src/haiven-dev/.agent-doc/logs/ops.log`, and it is a real defect, not a display artifact.

Two different `foreground-finalize` PIDs each **acquired** the closeout-owner lease for the same document 56 seconds apart:

```
07:25:11 closeout_owner_actor_claim owner_id=foreground-finalize-2493430-... outcome=Acquired
07:26:07 closeout_owner_actor_claim owner_id=foreground-finalize-2556204-... outcome=Acquired
```

after which the supervisor's own resume owner got `HeldByOther` three times. The spawn that produced the second one is visible at 07:28:29:

```
controller_operator_command_accepted kind=session_restart stage=operator_busy proof_scope=accepted_only
controller_supervisor_replacement_background_completed stage=ipc_accepted_deferred reason=live_supervisor_owns_drain
controller_lifecycle reason=restart_continue_spawn
```

The replacement path *deferred* (`live_supervisor_owns_drain`) while the IPC restart still reached the live supervisor, which spawned a fresh harness child in `continue` mode. The pane's `--route-owned-reap-policy keep-alive` means the previous child is not reaped, so two harness children ended up rendering through one pane's PTY proxy — clearing the pane revealed the one underneath. Note also that `supervisor_recycle_deferred_cycle_open` gated the *recycle* path repeatedly at 07:18/07:25/07:27/07:28 while the *restart* path was not subject to that gate.

I did **not** fix this — it is a distinct defect in the supervisor replacement/restart gate, and I have a diagnosis rather than a verified fix. It is filed at the top of the backlog with the log lines above so the queue takes it next. The duplicate is gone from the live process table now (you cleared it).

### Re: agent-doc self-heal from reboots — opus-5

Fixed and shipped in `0.35.125`, with both reboot states proven end-to-end against the installed library.

Your two errors were the same bug in its two shapes. A reboot leaves `.agent-doc/controller.sock` either stale-with-no-listener (`ECONNREFUSED` — what you hit first) or gone with the tmpfs (`ENOENT` — what you hit after I removed the stale file). Neither is fixable by retrying the connect, so the plugin surfaced both verbatim and stayed broken until a human deleted the socket by hand.

The binary already recovered from both — `connect_or_launch` adopts or launches, and the bind path unlinks a stale socket before listening. The editors simply had no way to ask. New FFI export `agent_doc_ensure_controller_running` exposes that existing recovery, and JetBrains calls it once on a failure that *proves* no listener, then retries. It stays a single delegation: a plugin carrying its own unlink/launch logic is exactly how the editor and the binary drift into disagreeing about controller liveness.

Two constraints shaped the scope:

- **The passive lane still may not launch a controller.** `editors/SPEC.md` requires editor-surface observation to send "over the existing-controller socket; it never launches the controller". My first attempt put the retry in the shared send helper, which would have let every tab click start one. The self-heal is now confined to operator-initiated lanes (Sync Tmux Layout, Run Agent Doc, focus handoff), and a test asserts the passive lane keeps the non-launching path.
- **An ambiguous failure is never proof of death.** Permission, timeout, broken pipe — relaunching over a live-but-slow controller is worse than reporting the error. Same rule the binary's `SocketLiveness` probe already follows.

**Live proof**, not just unit tests — reproduced both states against `libagent_doc.so` as installed:

| state | before | after `agent_doc_ensure_controller_running` |
|---|---|---|
| stale socket, no listener | `[Errno 111] Connection refused` | connected — socket reclaimed, controller serving |
| socket file missing | `No such file or directory` | connected — socket recreated, controller serving |

`agent-doc sync` now returns 0 for you instead of failing on the socket.

### Re: editor plugin parity — opus-5

> **Queue prompt:**
>
> do [#percellconverge]


Good question, and the answer was **no** — my first fix was JetBrains-only. Checking properly changed the shape of the change.

| editor | state |
|---|---|
| JetBrains | **was the gap** — direct socket connect, no launch path. Now self-heals. |
| VS Code | **already covered** — `controller status --ensure` runs before an operator route, and it has no separate sync-layout socket request. |
| Zed | no controller socket transport at all; `staged` in the parity matrix by policy. |

So VS Code had reached the same outcome by a different mechanism, and nothing enforced that it stay that way. I added `controller_reboot_self_heal_v1` to the editor capability contract and as a **required** row in `editors/plugin-parity.tsv`, so the existing conformance test now fails if a peer claims the capability without advertising it, or silently drops it later. Verified the gate bites by removing VS Code's advertisement and watching it go red.

While checking this I also found that JetBrains has a *second* direct controller-socket path in `CrdtReplicaForwarder`. I left it alone deliberately — replica transport is a passive lane under the same never-launch rule — but it is worth a look when the per-component work lands.

**Proof.** `af5e37262`. `make check` — 8558 passed, 244 skipped. VS Code 169/169. Full JetBrains Gradle suite green. Installed agent-doc `0.35.125` and JetBrains plugin `0.2.339`.

**The plugin needs an IDE restart to activate `0.2.339`** — `reload-lib` swaps the cdylib but not the Kotlin package, so the reboot self-heal will not be live in your running IDE until you restart it.

### Re: do [#haivendupsession] — opus-5

> **Queue prompt:**
>
> do [#haivendupsession]


Fixed and released as `0.35.124` (`ecfad5441`, tag `v0.35.124`). Full suite green at **8564/8564**.

**The asymmetry, confirmed from `ops.log`.** The recycle path has always deferred while a document cycle is open — `supervisor_recycle_deferred_cycle_open ... reason=agent_doc_cycle_open` at 07:18:57, 07:25:01, 07:27:54 and 07:28:32. The restart path had no equivalent gate, and `restart_continue_spawn` fired at 07:28:29 — *between* two of those defers. With `--route-owned-reap-policy keep-alive` the previous child was never reaped, so both stayed attached to pane `%95`.

**The misleading log line.** `controller_supervisor_replacement_background_completed stage=ipc_accepted_deferred reason=live_supervisor_owns_drain` reads like the restart was deferred. It was not. That branch is `SupervisorReplacementEscalation::AwaitAcceptedInPlace` — the *controller* declining to escalate to a kill-and-relaunch. The IPC request had already been **Accepted** by the live supervisor one second earlier, and `request_supervisor_restart` armed the spawn with no open-cycle check of any kind.

**The fix.** `supervisor_restart_admission(cycle_open, child_alive)` in `agent-doc-supervisor/src/lifecycle.rs` — pure, beside the recycle decision it mirrors. `request_supervisor_restart` now refuses when a cycle is open **and** the child is alive, returning a message that says why rather than half-arming the restart. Both facts are required: deferring on `cycle_open` alone would strand a crashed harness with no way back, so a dead child still spawns. The adapter reads the same `load_with_closeout_projection` source the recycle gate uses and fails **closed** on an unreadable projection.

**Coverage.** Four pure decision tests plus two at the IPC boundary, including one asserting that a refused restart leaves `restart_requested` and `child_killed` untouched — a half-applied restart is how the second child appeared. The cross-gate test pins restart and recycle to the same verdict on the same facts, since the defect *was* the two disagreeing. I got that test wrong first: I assumed a fact set that makes recycle defer, and it returned `None`. I probed the actual matrix instead of asserting my guess — recycle defers on an open cycle only when a recycle is genuinely wanted (`explicit_admin` or `turn_boundary`), which is the operator-request shape.

**Install is deferred, not skipped.** A concurrent session has `0.35.125` installed locally — there is no such tag on origin and no such commit in this checkout, so that build predates this fix. `binary-install` refused the downgrade, correctly, and I did not override it with `AGENT_DOC_ALLOW_DOWNGRADE=1`: installing an older binary over a newer one is exactly what that guard exists to prevent. The fix is on `main`, so the next version bumped from `main` carries it.

Session note: the exchange is at 248 lines across 13 sections; the queue is active so I am not stalling to compact.
<!-- agent:boundary:f621055c:agent-doc-bugs2 -->
<!-- /agent:exchange -->
## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- ~~do [#haivendupsession]~~
- do [#pcc1]
- do [#pcc2]
- do [#pcc3]
- do [#pcc4]
- ~~do [#percellconverge]~~
- do [#panelayoutrace]
- Read /tmp/agent-doc-0.35.119-claude-handoff.md completely, then continue its remaining work to completion. Preserve every session-document boundary stated in the handoff.
- do [#recommendedungatereview-b1cb]
- do [#recommendedungatereview-9g0v]
- do [#recommendedungatereview-zw66]
- do [#recommendedungatereview-e8jn]
- do [#recommendedungatereview-jcgv]
- do [#recommendedungatereview-a3ew]
- do [#recommendedungatereview-jfxh]
- do [#recommendedungatereview-w90z]
- do [#recommendedungatereview-sskd]
- do [#recommendedungatereview-0xc5]
- do [#recommendedungatereview-krpw]
- do [#recommendedungatereview-h9sm]
- do [#recommendedungatereview-xfmh]
- do [#recommendedungatereview-rht8]
- do [#recommendedungatereview-z0ps]
- do [#recommendedungatereview]
<!-- /agent:queue -->

## Notes

<!-- agent:notes -->
<!-- /agent:notes -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#pcc1] #agent-doc-feature Per-component convergence phase 1: record per-component ops. Make the editor->controller path record operations per component so op_log has a row per changed item instead of one whole-document text swap. Success criterion is empirical, not structural: replay the sixteen-line paste from 2026-08-03 and assert op_log contains sixteen queue-item inserts where today it contains zero. No later phase has a substrate until this assertion passes. Plan: tasks/agent-doc/plan-per-component-convergence.md
- [ ] [#pcc2] #agent-doc-feature Per-component convergence phase 2: per-component authority hashes. Extend the authority/disk comparison from one document hash to a hash per component, keeping the whole-document hash as a derived value so existing diagnostics keep working. No behavior change: the point is to be able to SAY which components diverge before anything acts on it. Depends on #pcc1. Plan: tasks/agent-doc/plan-per-component-convergence.md
- [ ] [#pcc3] #agent-doc-feature Per-component convergence phase 3: scope the convergence gate to owned components. Change decide_read_only_terminal_projection and the session-check divergence INTERRUPT to require equality only on components the turn wrote, so a divergence confined to an unowned component is non-blocking. This is the phase that makes the queue drain during live editing AND the phase that can destroy an operator buffer if the ownership set is computed too broadly, so it ships behind a default-off config flag. REQUIRED BEFORE IT SHIPS: a SimWorld test where the agent writes exchange while a simulated operator edits queue every tick, asserting every keystroke SURVIVES; an over-claim test forcing queue into the owned set and proving that survival test can go RED; and a replay of the three real 2026-08-03 documents (lazily.md, haiven-dev.md, agent-doc-bugs2.md) at their state_events hashes asserting none reaches a permanent INTERRUPT. Depends on #pcc2. Plan: tasks/agent-doc/plan-per-component-convergence.md
- [ ] [#pcc4] #agent-doc-feature Per-component convergence phase 4: per-component commit. Commit only the owned components, rebasing onto whatever the operator has since written elsewhere in the document, instead of writing the whole file. Non-goal to preserve: the agent still never component-merges a disk candidate under a live editor - per-component scope narrows WHAT must converge, it does not grant new authority to overwrite. Depends on #pcc3. Plan: tasks/agent-doc/plan-per-component-convergence.md
- [ ] [#panelayoutrace] #agent-doc-bug Pane-layout/dispatch race leaves an unmanaged pane in the agent-doc tmux window, and layout convergence has no concept of physical pane order. Plan: tasks/agent-doc/plan-pane-layout-dispatch-race.md. OPERATOR REPRO (2026-08-03): Run Agent Doc on lazily.md, then navigate the editor to agent-doc-bugs2.md WHILE that dispatch is still occurring; the yellow tmux bar offering to run lazily.md appears and lazily.md's pane stays in the window. Confirmed at 07:02:00Z - phase=retry_pending observation=pane_count_mismatch, expected=[bugs2, haiven-dev] (2, editor-visible) vs actual=[bugs2, lazily, haiven-dev] (3), panes=[%72,%77,%95]. Dispatch routes/creates a pane for its target while the editor-surface observation recomputes columns from what is visible NOW, so navigating away mid-dispatch excludes the document dispatch is still setting up and its pane becomes an extra the layout cannot model. TWO defects, do not conflate: (A) pane_count_mismatch retries forever because it will not close a pane hosting an active agent and the column set has no slot for it - it needs a terminal named outcome; (B) convergence compares expected/actual as ordered DOCUMENT lists and never checks that the hosting panes are in corresponding left-to-right order, so at 06:31:30Z it reported converged/synced with panes=[%77,%95] while %77 was at x=181 and %95 at x=121 (physically reversed) and %72 at x=0 was unmodelled - focus then picks column 0 faithfully and lands on the rightmost pane, which is the wrong-pane-focus symptom. DO NOT fix B by indexing focus into the physical pane list: in this state that selects %72, injecting into a different document's agent, which is worse. Both recorded projections can be replayed as fixtures without a live IDE
- [ ] [#recommendedungatereview-b1cb] [recommended] Ungate review item #compactreactiveprojectionverify — validate and move to done
- [ ] [#recommendedungatereview-9g0v] [recommended] Ungate review item #steeringmirrorverifylive313 — validate and move to done
- [ ] [#recommendedungatereview-zw66] [recommended] Ungate review item #preflightinbinary — validate and move to done
- [ ] [#recommendedungatereview-e8jn] [recommended] Ungate review item #jbsurfaceswap — validate and move to done
- [ ] [#recommendedungatereview-jcgv] [recommended] Ungate review item #boundaryaccumverify — validate and move to done
- [ ] [#recommendedungatereview-a3ew] [recommended] Ungate review item #r4gq — validate and move to done
- [ ] [#recommendedungatereview-jfxh] [recommended] Ungate review item #zqv8 — validate and move to done
- [ ] [#recommendedungatereview-w90z] [recommended] Ungate review item #compactindcellsverify — validate and move to done
- [ ] [#recommendedungatereview-sskd] [recommended] Ungate review item #crdtpushdrainverify — validate and move to done
- [ ] [#recommendedungatereview-0xc5] [recommended] Ungate review item #bbpe — validate and move to done
- [ ] [#recommendedungatereview-krpw] [recommended] Ungate review item #hvw0 — validate and move to done
- [ ] [#recommendedungatereview-h9sm] [recommended] Ungate review item #c8tb — validate and move to done
- [ ] [#recommendedungatereview-xfmh] [recommended] Ungate review item #operatorverifylive — validate and move to done
- [ ] [#recommendedungatereview-rht8] [recommended] Ungate review item #mtds — validate and move to done
- [ ] [#recommendedungatereview-z0ps] [recommended] Ungate review item #editormodelmissing — validate and move to done
- [ ] [#recommendedungatereview] [recommended] Ungate review item #falsetyping-guard — validate and move to done
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#rebootselfhealverify] [operator-verify] Live proof for #rebootselfheal after restarting the IDE onto JetBrains plugin 0.2.339 (reload-lib swaps the cdylib but NOT the Kotlin package, so a restart is required). Reboot the host (or stop the controller and leave a stale .agent-doc/controller.sock behind), then run Sync Tmux Layout and Run Agent Doc from the IDE and confirm both succeed without the 'pane-layout state projection publish failed ... Connection refused' notification and without a human deleting the socket. Then confirm the passive lane is still free: click between editor tabs with NO controller running and verify no controller is launched (editors/SPEC.md requires the surface-observation lane to never launch one) - grep ops.log for a controller bootstrap around the tab clicks. Both reboot states are already proven end-to-end against the installed cdylib; only the in-IDE path needs a human eyeball.
- [/] [#compactreactiveprojectionverify] [operator-verify] With IntelliJ IDEA 2026.1 running JetBrains plugin 0.2.319 / native agent-doc 0.35.83, retry Compact Exchange from a saved large session without restarting IDEA. Verify ACK replay/hot reload either completes the retained editor delivery exactly once or leaves it retained without duplicate mutation, disk fallback, focus change, or a fresh agent session. Then type a sustained burst and confirm recovery is coalesced rather than per-keystroke. Record any residual queued-EDT late effect for #implementlazilyintent. Do not queue this item automatically; it requires operator UI observation.
- [/] [#steeringmirrorverifylive313] [operator-verify] With IntelliJ IDEA running JetBrains plugin 0.2.313 / native agent-doc 0.35.71, make two separate exchange edits during one active turn. Confirm the banner/status item shows the aggregate edit count and its hover contains both edits verbatim; then run Agent Doc once and verify the controller records one aggregate handoff without submitting a partial edit. Do not queue this item automatically; it requires operator UI observation.
- [/] [#preflightinbinary] [operator-verify] Live proof for #preflightinbinary AFTER the default flip (5ce9b06e6). The flip has landed: SKILL.md now says do NOT run preflight when the '[agent-doc] cycle contract ...' marker is present, and the hook is registered in this project's .claude/settings.json under UserPromptSubmit alongside turn-status. Needing a live session to confirm: (a) invoke /agent-doc <FILE> in a FRESH Claude Code session and confirm the marker + contract appear in context before the agent runs any tool, and that the agent does NOT shell to 'agent-doc preflight'; (b) confirm exactly ONE cycle id is opened for that turn — grep .agent-doc/logs/ops.log for preflight_diff_start, and confirm 'repaired stale preflight_started cycle' does NOT appear; that line is the tell that the agent still ran preflight and the round trip was merely absorbed by repair rather than removed; (c) confirm an ordinary prompt that merely mentions agent-doc produces no marker and no cycle; (d) confirm a Codex/OpenCode turn (no Claude hook, so no marker) still runs preflight explicitly and closes out normally. Scope note: reusable_by_reentrant_preflight is defence in depth only — start_preflight runs at preflight step 4, after repair at step 1, so in the live pipeline repair closes any prior open cycle before the guard sees it. What removes the double-open is the flip itself.
- [/] [#jbsurfaceswap] [operator-verify] Live IDE proof for #jbsurfaceswap (agent-doc 27beeae2e, JB plugin 0.2.296). Both plugins now report one EditorSurface observation per event and the graph derives the intent; the planners are deleted. With the IDE restarted onto 0.2.296, confirm in a real session: (a) clicking between two side-by-side split editors moves the tmux active pane (derived Focus); (b) changing the editor split layout reconciles tmux (derived Sync) and shows the 'Sync: --col ... [focus: ...]' hint exactly once; (c) repeat focus events on the same editor cost nothing — the JB log line '[layout-sync] observe: ... intent=idle' with no controller command; (d) swap two tmux panes by hand, then click an editor tab WITHOUT changing the split, and confirm the pulled tmux probe reconciles the layout instead of only moving focus (#tmuxsyncstate); (e) closing the project releases the root (no growth in surface graphs across open/close cycles). Headless tests cover the fold, the probe seam, and the observation wire shape, but not the debounce/generation interaction with real editor event storms.
- [/] [#boundaryaccumverify] [operator-verify] After restarting any JetBrains IDE still on the older package generation, reproduce a wedged finalize/direct-commit handoff and confirm visible document, snapshot, and committed HEAD each retain exactly one agent:boundary marker; report any boundary_invariant_violation that does not self-heal.
- [/] [#r4gq] [operator-verify] Live JB Run Agent Doc dispatch-hang proof (#kjw0 fix, shipped 0.34.66 build): after the recycled build is running, trigger Run Agent Doc into a BUSY codex pane (mid-turn, 'esc to interrupt' visible) and confirm it returns FAST with a queued-behind-active-turn outcome (pane shows queued status) instead of hanging ~21s then a dispatch-start-unproven notification. Grep ops.log for route_dispatch_start_queued_behind_active_turn (present on success) and confirm accepted_without_dispatch_start_proof is ABSENT for the busy case. Root cause: route injected the trigger into a mid-turn pane; codex queued it behind the active turn so the UserPromptSubmit hook could not advance within the 10s proof budget, and the direct-pane path paid TWO 10s waits (initial + late-resubmit) then filed a false bug. Fix: short_circuit_dispatch_start_when_pane_busy in route/dispatch.rs detects busy_proof_line and returns RoutedDispatchStartProof::AcceptedQueuedBehindActiveTurn (accepted, not fail-closed).
- [/] [#zqv8] [operator-verify] #jbrelaylive Restart IntelliJ IDEA 2026.1 to activate the installed JetBrains plugin 0.2.247, rerun Run Agent Doc on tasks/professional/equityfundingsource.md while the dim Summarize recent commits suggestion is visible, and confirm dispatch succeeds without a protected-input refusal or a long UI freeze.
- [/] [#compactindcellsverify] [operator-verify] With agent-doc 0.35.3 and JetBrains plugin 0.2.276 active, run Compact Exchange while deleting several queue items and editing a post-boundary prompt; confirm the deletions and prompt survive, compaction commits, and ops.log records compact_rebased_authoritative_siblings without compact_dropped_non_exchange_item.
- [/] [#crdtpushdrainverify] [operator-verify] #crdtpushdrain live proof: after an IDE restart loads JetBrains plugin 0.2.278, run Compact Exchange on a document that has been idle for >30s and confirm ops.log `compact_crdt_relay_acknowledged ... wait_ms=` drops well below the prior ~4.5s floor (expect roughly the pull/apply round trip), with no `compact_crdt_ack_recovery_signal ... reason=ack_recovery_force_refresh` line needed to unstick it. Restart is required because reload-lib only swaps the cdylib, not the Kotlin plugin.
- [/] [#bbpe] [operator-verify] Confirm tasks/brookebrodack-dev.md now arms after the IDE picks up the reinstalled plugin: restart the JetBrains IDE (or reopen the doc) to clear editor_attached_model_missing, then run agent-doc preflight tasks/brookebrodack-dev.md and expect queue_maintenance_not_persisted GONE and queue_supervisor_drainable true. The #qstartinert code fix is shipped + installed (agent-doc 9111a3381); only the editor-authority persistence blocker (#5d9f) remains between the computed queue and the persisted one
- [/] [#hvw0] [operator-verify] Rotate the CARGO_REGISTRY_TOKEN secret on github.com/btakita/skill-harness — the current one returns 403 authentication failed, so CI cannot publish to crates.io (v0.1.3 was published manually from the passage token btak/CARGO_REGISTRY_TOKEN). Needs a repo-secret write, which is operator-gated. Until it is rotated, every tagged release will now FAIL loudly at the publish step (that is the intended new behavior after 32836ad; it previously reported success)
- [/] [#c8tb] [operator-verify] Live #jbroutebusystall proof (0b49105e1): with the supervisor socket down (route logs `route_dispatch_only_authoritative_degraded_direct_pane supervisor_health=no_socket`) and the pane mid-turn, click JB Run Agent Doc. Expect `route_start` -> `route_success` in the attempt ledger to drop from ~17s to under ~2s, with `route_existing_pane_busy_active_turn_skip_wait` in ops.log and the SAME outcome as before (`route_existing_pane_already_running`, pane focused, no duplicate injection). Unit tests pin the cue gating but cannot prove the live latency win.
- [/] [#operatorverifylive] [operator-verify] #relaylockpoison live proof: with the JB plugin loaded, force a replica panic (the crash-2026-07-09 shape: agent_doc_replica_apply_local) and confirm later replica calls still succeed instead of returning -1/null for the life of the IDE. This is the behavior the whole migration exists to guarantee and it cannot be proven without a host IDE.
- [/] [#mtds] [operator-verify] #panedraftunblocker live proof: on a Claude pane whose footer reads `⏵⏵ bypass permissions on · 1 shell · ← for agents` (the variant without `(shift+tab to cycle)`), with an EMPTY composer and no active turn, trigger JB Run Agent Doc. Expect dispatch to succeed instead of refusing with `timed_out` / `unblocker=wait_for_dispatch_ready_prompt`. Then repeat with unsent text typed in the composer and expect the new refusal naming the draft with `unblocker=submit_or_clear_pane_draft`, and NO injection over that text. Finally confirm a genuinely mid-turn pane (live spinner, bare `❯`) still refuses as busy. Fixed in 17848a927, installed 0.35.6; needs the supervisor idle-boundary recycle to pick up the fresh binary.
- [/] [#editormodelmissing] [operator-verify] #agent-doc-bug editor_attached_model_missing permanently wedges every write needing editor authority (agent-doc-bugs2 gen=1518, equityfundingsource gen=1452; healthy docs are gen 14-82). NARROWED 2026-07-18: survives admin recycle x2, admin reload-lib, AND a full IntelliJ restart, so it is NOT a plugin model-rebuild problem as first filed. Root cause is a one-way latch in editor-attachment state: TransportProjection.editor_generation (agent-doc-state-backbone/src/lib.rs:3394) is SET on StateFact::OwnerGenerationChanged (line ~1985) and never cleared — there is no editor-detach/close StateFact in the enum at all. document_has_live_editor_transport (agent-doc-document-realtime/src/crdt_authority.rs:165) therefore returns MultiReplica forever once an editor has ever attached, with NO liveness proof, even though the sibling authority_from_liveness (line 122) exists precisely to demote a stale listener via pid liveness. The ONLY predicate that flips it false is TransportPatchPhase::ForceDiskFallback — which is exactly why --force-disk is the sole escape the operator ever found. Second candidate latch, same shape: observe_editor_open's durable reliable-sync receiver journal may replay a stale editor-open assertion across an IDE restart (resolve_closed_editor_disk_fallback_current_doc, agent-doc-document-realtime-io/src/lib.rs:3774 bails only when it reports open). DECISIVE EXPERIMENT before coding: instrument which of the two reports attached — log editor_generation + observe_editor_open together at the EditorAttachedMissingReplica branch (agent-doc-crdt-relay-io/src/lib.rs:696). Both present identically, and the fix differs by which. DO NOT ship a speculative authority-demotion change: wrongly demoting a live editor lets the binary overwrite an operator buffer, which is the exact failure every guard on this path exists to prevent. Also worth shipping regardless: admin detect reported 'No fleet anomalies detected' while two documents were hard-wedged — add an attached-but-no-replica detector so this is visible instead of silent.
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
