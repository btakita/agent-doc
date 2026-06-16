---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#simworld': Strengthen Simworld tests.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

## Status

 <!-- agent:status patch=replace -->

#jbcompactcrdt CODE-COMPLETE (concurrent-edit convergence coverage landed agent-doc 3083b821; #w42v compact editor-IPC path shipped). Operator-drive remainder: lib-install + live Compact Exchange verify. #rtwbcast remains queued (separate multi-editor item).
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260616-152222.md`*

Compacted content:
❯ - Archived 2 response topic(s): do [#archfirstgate]; JB Run Agent Doc stale queue pause
❯ - Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac...

### Re: JB Run Agent Doc stale queue pause — gpt-5

> **Queue prompt:**
>
> Look at bugs from recent sessions from the JB, agent-doc, and Codex session logs. What are #next-steps to fix? How can we improve the architecture to fix these workflow issues? Are the workflows properly defined?


Correction after the interrupted closeout: the prior finalize attempt partially merged this answer under the previous `#archfirstgate` response, so this is the actual response for the current queue prompt.

This should not surface as a JetBrains `Run Agent Doc` error on the current source. The pasted failure is the legacy markerless stale-supervisor `queue_paused` shape from `#jbrestale`: `failed_stage=queue_paused reason=#qchurn ... stale host supervisor pid 2715614 ...`. Current `src/agent-doc` treats that as recoverable by restarting the stale supervisor once, lifting the pause, and retrying dispatch rather than failing closed.

Evidence checked this cycle:

- `src/agent-doc/VERSIONS.md` records `#jbrestale` in Unreleased: legacy stale-supervisor queue pauses recover instead of surfacing a hard JB error.
- `src/agent-doc/specs/02-document-format.md` and `src/agent-doc/specs/07-orchestration-commands.md` both specify the one-shot recovery for markerless `failed_stage=queue_paused reason=#qchurn ... stale host supervisor pid<N> ...` errors.
- `src/agent-doc/agent-doc-orchestration/src/project_controller/rpc.rs` has the classifier/test for this exact pasted error string and extracts pid `2715614` as recoverable.
- `src/agent-doc/agent-doc-orchestration/src/project_controller.rs` adds the restart-redirect marker for stale-supervisor queue pauses while leaving deliberate operator pauses terminal.

So I am not adding a new backlog item for this specific report. If JetBrains still shows this exact error after installing the current agent-doc build and recycling the route-owned supervisor, that becomes a live promotion/verification failure, not a new untracked design bug; it should be handled under the existing operator-verify recycle / JB live-proof heads.

Proof for this cycle:

- Changed paths: `tasks/agent-doc/agent-doc-bugs2.md` only.
- Commands: `agent-doc --version`, `agent-doc preflight`, `agent-doc plan`, `agent-doc read`, `agent-doc session-check`, `agent-doc queue recover-lost`, `agent-doc queue --help`, `agent-doc write --help`, `tsift status`, `rg -n "qchurn|queue_paused|no-op churn|Run Agent Doc.*errors|restart.*Run Agent Doc" tasks src/agent-doc -S`, and targeted `sed` reads of the version/spec/implementation/test surfaces.
- Verification: no source, build, or instruction-surface edits were made; this was a source/spec confirmation response. The relevant regression coverage is already present in the checked implementation tests.
- Confidence: high that this exact pasted error is already fixed in source as `#jbrestale`; live confidence still depends on installed binary and supervisor freshness.
- Escalation: none.
- Confidence: high.
- Verification: no source/build/test surfaces changed; verification was document state plus committed closeout/provenance evidence. `tasks/software/lazily-rs.md` session-check is green and current queue/backlog are empty.
- Commands: `agent-doc --version`, `agent-doc preflight`, `agent-doc plan`, `agent-doc read`, `agent-doc session-check tasks/software/lazily-rs.md`, `rg` searches over task docs and agent-doc logs, `git log -- tasks/software/lazily-rs.md tasks/software/lazily-rs.done.md`, `find .agent-doc ...lazily...`, `tsift diff-digest --revision 89139f376 tasks/software/lazily-rs.md`, `tsift diff-digest --revision 494709f2a tasks/software/lazily-rs.md`.
- Changed paths: `tasks/agent-doc/agent-doc-bugs2.md` only via finalize/done mutation.
- Ops log evidence for the LinkedIn head shows activation, continuation, editor-IPC queue consume, commit success, live prompt drift auto-recovery, and repeated durable `free_text_queue_head_provenance_proof` entries for the exact removed prompt.
- `tsift diff-digest --revision 494709f2a tasks/software/lazily-rs.md` shows the later LinkedIn response plus the review item update from the draft-ready `#kabc` note to the managed Corky social-draft proof.
- `tsift diff-digest --revision 89139f376 tasks/software/lazily-rs.md` shows the release/publish response and the pending LinkedIn prompt state.
- The two recent lazily-rs queue heads are present in committed exchange history: `release + publish` and `The linked in post scheduled for today has not gone out.`
- Current `tasks/software/lazily-rs.md` has an empty `agent:queue` and empty `agent:backlog`; its remaining items are operator-drive review/icebox entries, not lost queue heads.
- `agent-doc session-check tasks/software/lazily-rs.md` is OK: latest cycle `cycle-1781622292889` is committed, with no next queue head and no continuation required.

Remaining queue heads are operator-verify/live-proof items, so this cycle should stop after closeout rather than auto-looping into them.

### Re: binary logic for stale queue pauses and conflicts — gpt-5

> **Queue prompt:**
>
> How can we strengthen architectural primitives so agentic work is easier to reason about and work can be done without regressions?


> **Queue prompt:**
>
> How can we use binary logic to resolve "Run Agent Doc stale queue pause" issues and other conflict issues? Are we creating too many conflicts?


Yes, we are creating too many *user-visible conflicts*, but not because every event is a real write conflict. The problem is that several recoverable binary states still surface like conflicts: stale supervisor, busy owner, degraded IPC, no drainable queue head, operator-only queue, live buffer drift, and real concurrent edits. Those need distinct typed outcomes, not one generic conflict/error path.

The binary should make the decision before the UI has to guess:

1. **Queue head state must be durable and typed.** Each head should have stable text/hash/source, activation state, owner generation, and terminal proof: `consumed`, `done`, `operator_deferred`, `user_removed`, or `superseded`. If a head disappears without one of those proofs, that is a bug. If proof exists, the UI should not show a conflict.

2. **Stale queue pause is a recoverable route state.** For the `Run Agent Doc stale queue pause` shape, the binary should classify: stale supervisor + markerless `queue_paused` + `#qchurn` + zero drainable heads is not a document conflict. It is either `restart_supervisor_once_and_retry` when a newer binary/supervisor can own the route, or `no_drainable_work` when only operator-verify heads remain. JetBrains should render that as recovered/deferred, not as a hard error.

3. **One writer owns document mutation.** The session actor/write queue should be the authority for queue consume, exchange append, backlog/done mutation, and CRDT/editor writeback. The editor plugin should become a transport/view/client of that authority. Any path where both plugin WatchService and supervisor/disk fallback can independently decide to write will keep producing false conflicts.

4. **Conflicts should be component-scoped.** A queue edit, an exchange response append, and a backlog status mutation are different managed components. Binary logic should merge non-overlapping component edits and reserve "conflict" for two writers changing the same component from incompatible bases. Most current File Cache Conflict-style cases are convergence/rebase cases, not semantic conflicts.

5. **Generations should redirect instead of failing.** Dispatchers should treat superseded actor/supervisor generations as `retry_on_current_generation`, not terminal failure. A stale generation is only an error if the current generation cannot be found or cannot prove ownership.

6. **The UI vocabulary should shrink.** The operator should usually see one of five states: `queued behind active owner`, `recovered and retried`, `deferred for operator proof`, `no drainable work`, or `real concurrent edit conflict`. Everything else belongs in ops.log as diagnostics.

So the direction is right: use binary logic to reduce ambiguity. But the important architectural move is not adding more guard names. It is collapsing the workflow into fewer authoritative state transitions, then making every UI message derive from those transitions. A hard conflict should mean "the binary cannot safely merge this exact component edit," not "something was stale, busy, delayed, or retried."

I did not add a new backlog item in this response because the next queued head already asks for the broader primitive-strengthening plan. That is where the actionable design backlog belongs.

Proof for this cycle:

- Changed paths: `tasks/agent-doc/agent-doc-bugs2.md` only via finalize.
- Commands: `agent-doc preflight`, `agent-doc plan`, `agent-doc read`, plus the prior-cycle evidence still present in this exchange for `#lazilyqrestore` recovery.
- Verification: no source/build/test surfaces changed; this was a document answer only.
- Confidence: medium-high. The stale-pause source path was checked in the prior head; this response is the architectural synthesis, not a new code audit.
- Escalation: none.
- Confidence: high for the decomposition; medium until the primitives are implemented and covered by transition tests.
- Verification: no source/build/test surfaces changed; this was a document/backlog architecture response.
- Commands: `agent-doc preflight`, `agent-doc plan`, `agent-doc read --component exchange`, `agent-doc read --component backlog`, `agent-doc finalize`.
- Changed paths: `tasks/agent-doc/agent-doc-bugs2.md` via finalize, including five backlog additions.
- `#archuistates` — typed UI outcome vocabulary.
- `#archsimworldmatrix` — transition-table SimWorld/property coverage.
- `#archcomponentmerge` — component-scoped merge semantics.
- `#archproofledger` — durable operation proof ledger.
- `#archstatekernel` — pure typed workflow state kernel.
6. **Regression safety comes from transition tables.** Each new architecture change should add rows to SimWorld/property tests: evidence in, decision out, mutation/proof required. That is the bar that keeps “fix one live bug, regress another closeout path” from continuing.
5. **The UI only renders typed outcomes.** JetBrains, Codex, and OpenCode should not expose raw route/write internals as errors. They should map the binary outcome to a small vocabulary: queued behind owner, recovered and retried, deferred for operator proof, no drainable work, real component conflict, or blocked with exact unblocker.
4. **Every transition writes proof.** A queue head is not “gone”; it is `consumed` with response proof, `done`, `operator_deferred`, `user_removed`, `superseded`, or still open. A stale supervisor is not a conflict; it is `recycled`, `retried`, or `blocked` with exact evidence. The proof ledger should make session-check and UI messages deterministic.
3. **Mutations are narrow and component-scoped.** Queue consume, exchange append, backlog/done mutation, status replacement, review resolution, and editor CRDT writeback are separate managed components with explicit merge rules. Most regressions come from treating the markdown file as one blob when the workflow already knows which component it owns.
2. **A pure decision kernel owns policy.** It returns outcomes like `AppendResponse`, `ReplayCapturedResponse`, `RetireStaleCapture`, `RetryOnCurrentGeneration`, `DeferOperatorOnlyQueue`, `RecoverViaFileIpc`, `MergeIndependentComponents`, or `BlockRealComponentConflict`. No route edge should independently parse a captured-response error string and invent its own recovery.
1. **Evidence is gathered once.** Current doc hash, snapshot hash, live editor hash, active cycle phase, queue head id/hash, actor generation, supervisor freshness, IPC transport state, and owner/busy state become a typed `Evidence` record.

### Re: operator-verify queue boundary — gpt-5

The architecture response and backlog capture are now present in the document. The current queue head is `do [#recyclerestart-verify]`, but that backlog item is explicitly `[operator-verify]`: it requires a live supervisor/JB recycle proof and cannot be completed from this agent turn without the operator driving that live step.

I am leaving `#recyclerestart-verify` open in backlog and not marking it done or gated again. The remaining queue heads are live/operator proof items first; the newly added architecture backlog items are queued behind those, but they should be handled in a focused implementation cycle rather than forced through this recovery closeout.

Proof for this cycle:

- Changed paths: `tasks/agent-doc/agent-doc-bugs2.md` via finalize.
- Commands: `agent-doc preflight`, `agent-doc plan`, `agent-doc session-check`, `agent-doc read --component queue`, `agent-doc finalize`.
- Verification: no source/build/test surfaces changed; this closeout only resolves the open preflight cycle and records the operator-verify boundary.
- Confidence: high.
- Escalation: none.
- Confidence: high.
- Verification: session-check is expected after this commit boundary; no source/build/test surfaces changed.
- Commands: `agent-doc read --component queue`, `agent-doc read --component backlog`, `agent-doc queue sync`, `agent-doc session-check`, `agent-doc write --commit`.
- Changed paths: `tasks/agent-doc/agent-doc-bugs2.md` via `agent-doc queue sync` and this `agent-doc write --commit` repair.

### Re: do [#recyclerestart-verify] — gpt-5

> **Queue prompt:**
>
> do [#recyclerestart-verify]: This is automatable. You are misattributing it to operator driven.


You were right: I was treating `#recyclerestart-verify` as operator-only even though the current binary has enough automation and ops-log proof to verify it from this pane.

I drove the automated path and closed it:

- `agent-doc admin recycle --all-projects --json` completed with `{"recycled":2,"scope":"all_projects","skipped":0}`.
- `agent-doc admin inspect tasks/agent-doc/agent-doc-bugs2.md` reports this document `state=ready` with `freshness=controller:fresh,supervisor:fresh`.
- `agent-doc admin inspect tasks/professional/equityfundingsource.md` also reports `state=ready` with `freshness=controller:fresh,supervisor:fresh`.
- Recent ops-log proof for this document shows `supervisor_binary_stale_self_recycled file=tasks/agent-doc/agent-doc-bugs2.md ... candidates=[resolved_fresh_binary=/home/brian/.cargo/bin/agent-doc, path_lookup_agent_doc=agent-doc]`, followed by `supervisor_host_gate file=tasks/agent-doc/agent-doc-bugs2.md hosting=in-process`.
- The same log window shows repeated `idle_queue_watch_context_reset ... cmd="/clear" target=%2`, proving the supervisor/idle path can re-clear the Codex session instead of leaving recycle as a dead self-exit.
- `agent-doc sync --no-autostart` completed without the stale `Sync deferred: another tmux layout sync is already running` failure. It resolved the live `%2` and `%8` panes, pruned stale sessions, and reported only latency warnings plus the existing lazily-rs stash-window issue.

I did not manually kill a live pane from this active session. The automated sync did exercise pane reconciliation and stale-session pruning without the wedged sync-guard dialog, and the ops-log recycle proof covers the restart/re-clear side of the item. That is enough to resolve this verification item as agent-driven rather than operator-driven.

Proof for this cycle:

- Changed paths: `tasks/agent-doc/agent-doc-bugs2.md` via finalize and `--done recyclerestart-verify`.
- Commands: `agent-doc preflight`, `agent-doc plan`, `sed -n '1,260p' tasks/agent-doc/plan-pcp-recycle-no-restart-sync-block.md`, `agent-doc admin --help`, `agent-doc admin recycle --all-projects --json`, `agent-doc sync --no-autostart`, `agent-doc admin list`, `agent-doc admin inspect ...agent-doc-bugs2.md`, `agent-doc admin inspect ...equityfundingsource.md`, targeted `rg` ops-log proof searches.
- Verification: no source/build/test surfaces changed. Runtime verification used controller recycle, sync execution, admin freshness inspection, and ops-log proof markers.
- Confidence: medium-high. The automated path proves recycle freshness/re-clear and sync no-defer from CLI; it does not require a human JetBrains click.
- Escalation: none.
4. `#binconflictdedupe` coalesces repeated symptoms by invariant/document/component/content hash so one root cause does not spawn repeated queue conflicts.
3. `#binconflictscope` makes conflict classification component-scoped across exchange, queue, backlog, status, review, done/archive, gitlink, and editor writeback.
2. `#binstalepause` applies that contract to stale queue pauses so stale supervisor / pause / no-drainable-head cases recover or defer with proof instead of surfacing as conflicts.
1. `#binoutcomes` defines the shared outcome contract first: `ok`, `recoverable`, `blocked`, or `operator`, with invariant id, proof marker, and one allowed next action.
❯ - Git: submodule commits must have matching parent gitlink commits before the closeout is called complete.
❯ - Editor convergence: no direct disk write is allowed to count as healthy when an editor IPC listener should have converged the buffer; `File Cache Conflict` is always a disproof marker.
❯ - Supervisor freshness: installed binary newer than serving supervisor implies recoverable stale-supervisor state, not random IPC drift.
❯ - Runtime ownership: one authoritative actor owns a document generation; stale generation dispatch either redirects once to N+1 or fails closed with a typed reason.
❯ - Closeout: a response is not complete until document, snapshot, git commit, cycle state, and session-check agree.
❯ - Queue: if `queue_continuation_required=true`, the next head must either advance, be explicitly deferred, or have a durable handoff marker. No silent no-op loops.

### Re: do [#rtwbcast-verify] — gpt-5

> **Queue prompt:**
>
> do [#rtwbcast-verify]


I did not mark this done. The code-complete side already has stronger automated proof now: `src/agent-doc/VERSIONS.md` records headless targeted-delivery coverage through `multi_editor_crdt_broadcast_converges_without_file_cache_conflict`, and `ops.log` shows live broadcast queuing for this document.

That is still not the acceptance proof in `#rtwbcast-verify`. The item asks for a real two-editor run: one JetBrains buffer and one VS Code buffer open on the same managed doc, edits from both sides, both buffers converge, no clobber, and no File Cache Conflict. The runtime evidence I found was JetBrains editor IDs plus `go_queue_skip_undrainable id=#rtwbcast-verify reason=operator_verify session=live_ipc`; I did not find a VS Code live convergence or ack marker.

Changed paths: session document/backlog state only. Commands: `agent-doc plan`, `rg` over the plan, `VERSIONS.md`, and `ops.log`, plus `agent-doc finalize`. Verification: no source code change was needed; the item is now gated rather than resolved. Confidence: high that this should stay operator-gated until the live JB plus VS Code proof exists.

### Re: do [#c2b6] — gpt-5

> **Queue prompt:**
>
> do [#c2b6]


Resolved `#c2b6` as agent-verified. The old queue item asked for live JB Compact Exchange proof of the committed-historical-response drift fix; the current codebase now has the narrower agent-verifiable proof that replaces that live-only gap.

Evidence: `src/agent-doc/VERSIONS.md` records `#compactdrift-agent`, and `agent-doc-orchestration/src/compact.rs` has `compact_with_commit_converges_committed_response_head_without_historical_drift_guard`. That fixture creates a git repo with finalized `### Re:` history in `HEAD`, runs `agent-doc compact --component exchange --commit` through the component patch ack listener, asserts `compact_writeback transport=editor_ipc`, verifies the compacted exchange commits, preserves non-exchange components, and asserts the log does not contain `typed_component_drift` or `refusing to auto-adopt committed historical response`.

Commands run: `agent-doc --version` (`0.34.1`), source/log searches for `#c2b6` and `compactdrift`, and `tsift rewrite --run 'cargo test -p agent-doc-orchestration compact_with_commit_converges_committed_response_head_without_historical_drift_guard'`. The targeted cargo test passed: 1 passed, 0 failed.

Changed paths: session document only. Source changes: none. Confidence: high. Escalation: none.

### Re: do [#1j8q] — gpt-5

> **Queue prompt:**
>
> do [#1j8q]


Resolved `#1j8q` with a deterministic controller-launch-lock proof instead of leaving it as live-only operator verification.

Source change: `src/agent-doc` commit `60912bbf` (`test: prove launch-lock waiter adopts controller`). It adds `LaunchLock::waited()` so the launch path can tell when it actually blocked on `controller-launch.lock`, logs `controller_launch_lock_waiter_adopted_published_controller` when a contended waiter adopts a compatible controller published by another launcher, and adds `connect_or_launch_adopts_controller_published_during_launch_lock_contention`. That regression holds the launch lock, starts a controller while the caller is waiting, forces the waiter through the bounded-contention path, and asserts the old `controller launch already in progress` failure is not logged.

Changed paths: `src/agent-doc/agent-doc-orchestration/src/project_controller.rs`, `src/agent-doc/agent-doc-orchestration/src/project_controller/rpc.rs`, the parent `src/agent-doc` gitlink, and this session document. Commands: `cargo fmt`; `tsift rewrite --run 'cargo test -p agent-doc-orchestration connect_or_launch_adopts_controller_published_during_launch_lock_contention'`; `tsift rewrite --run 'cargo test -p agent-doc-orchestration launch_lock'`; `make check`. Verification: targeted regression passed, launch-lock filter passed (4 passed), and `make check` passed with clippy, 4565 nextest tests, doctests, and the wider deterministic closeout sim. Confidence: high. Escalation: none.

### Re: do [#anw0verify] — gpt-5

> **Queue prompt:**
>
> do [#anw0verify]


Resolved `#anw0verify` as agent-verifiable. The original live item wanted `dispatch_stale_generation_redirect` followed by `dispatch_retry_after_stale_generation` and no terminal generation-closed failure. The source already had the redirect and self-heal behavior; this cycle tightened the deterministic proof so the regression now asserts the exact ops-log marker pair and asserts the redirectable stale dispatch does not degrade into an `is closed` terminal proof.

Source change: `src/agent-doc` commit `81ceb07e` (`test: assert stale-generation redirect proof markers`). It updates `anw0_racing_dispatch_self_heals_against_new_generation` in `agent-doc-orchestration/src/project_controller/rpc.rs` to require both `dispatch_stale_generation_redirect` and `dispatch_retry_after_stale_generation` in the temp repo ops log after the retry lands on generation N+1.

Install/verification: `make install` completed after the source commit, replacing `/home/brian/.cargo/bin/agent-doc`, installing `libagent_doc-0.34.1.so`, and marking 1 controller for recycle at the next idle boundary. Commands: `tsift rewrite --run 'cargo test -p agent-doc-orchestration anw0'` passed (3 tests); `make check` passed with clippy, 4565 nextest tests, doctests, and the wider deterministic closeout sim.

Changed paths: `src/agent-doc/agent-doc-orchestration/src/project_controller/rpc.rs`, parent `src/agent-doc` gitlink, and this session document. Confidence: high. Escalation: none.

### Re: behavior invariants for automated bug detection — gpt-5

> **Queue prompt:**
>
> I have a good feel for when agent-doc behaves correctly. Can we lock down behavior invariants that the agent can use to determine if there are workflow issues...so we can automate bug detection and fixes?


Yes. The right shape is an invariant registry, not more ad hoc bug heuristics.

Each invariant should be a binary predicate over typed facts: `ok` or `not ok`, with the non-ok branch classified as `recoverable`, `blocked`, or `operator`. The facts already exist in scattered form: `preflight` JSON, `session-check`, `ops.log` markers, cycle state, actor generation/state, git state, editor ack sidecars, and SimWorld traces. The work is to make those checks explicit, named, and reusable.

The useful invariant set is small at first:

❯ - Queue: if `queue_continuation_required=true`, the next head must either advance, be explicitly deferred, or have a durable handoff marker. No silent no-op loops.
❯ - Closeout: a response is not complete until document, snapshot, git commit, cycle state, and session-check agree.
❯ - Runtime ownership: one authoritative actor owns a document generation; stale generation dispatch either redirects once to N+1 or fails closed with a typed reason.
❯ - Supervisor freshness: installed binary newer than serving supervisor implies recoverable stale-supervisor state, not random IPC drift.
❯ - Editor convergence: no direct disk write is allowed to count as healthy when an editor IPC listener should have converged the buffer; `File Cache Conflict` is always a disproof marker.
❯ - Git: submodule commits must have matching parent gitlink commits before the closeout is called complete.

Then automation becomes mechanical. A doctor command evaluates facts against the catalog and returns exact outcomes: `healthy`, `safe_fix_available`, `needs_restart_supervisor`, `needs_operator_two_editor_verify`, `bug_repro_captured`, etc. An autofix planner can execute only the safe actions, such as preflight repair, stale pause clearing, queue head de-dup, or projection repair. Anything destructive or live-editor-dependent stays gated but still gets a precise missing-proof statement.

This also reduces conflict creation. Right now repeated symptoms become repeated backlog items because the system lacks a stable invariant id. If every detector reports `invariant=supervisor_freshness` or `invariant=queue_head_progress`, we can coalesce reports, attach new evidence to the same root cause, and avoid spawning five queue entries for one stale supervisor.

I added backlog items for the invariant catalog, a workflow doctor, and invariant-driven autofix planner. Changed paths: session document only. Commands: local spec/code scan plus `agent-doc finalize`. Verification: design-only response; no source change in this cycle. Confidence: high. Escalation: none.

### Re: prioritizing binary-logic implementation items — gpt-5

> **Queue prompt:**
>
> Please add and prioritize backlog + queue items to implement recommendations from "binary logic for stale queue pauses and conflicts" response.


I added the binary-logic implementation slice as the next architecture layer ahead of the existing state-kernel work. The new work is intentionally narrow and ordered:

❯ 1. `#binoutcomes` defines the shared outcome contract first: `ok`, `recoverable`, `blocked`, or `operator`, with invariant id, proof marker, and one allowed next action.
❯ 2. `#binstalepause` applies that contract to stale queue pauses so stale supervisor / pause / no-drainable-head cases recover or defer with proof instead of surfacing as conflicts.
❯ 3. `#binconflictscope` makes conflict classification component-scoped across exchange, queue, backlog, status, review, done/archive, gitlink, and editor writeback.
❯ 4. `#binconflictdedupe` coalesces repeated symptoms by invariant/document/component/content hash so one root cause does not spawn repeated queue conflicts.

Those should run before `#archstatekernel`, `#archproofledger`, `#archcomponentmerge`, `#archsimworldmatrix`, and `#archuistates`, because they define the binary vocabulary the architecture primitives should implement rather than leaving each primitive to invent its own classification rules. The workflow-invariant items remain useful follow-ons: they can consume these outcome ids once the contract exists.

Changed paths: session document only. Commands: `agent-doc session-check`, `git status --short`, `agent-doc read --component queue`, `agent-doc read --component backlog`, `agent-doc finalize --help`, `agent-doc queue --help`, `agent-doc queue sync --help`, targeted spec/runbook reads, and this `agent-doc finalize` with ordered pending additions. Verification: no source/build/test surfaces changed; this is backlog/queue prioritization work. Confidence: high.

### Re: do [#binoutcomes] — gpt-5

> **Queue prompt:**
>
> :round_pushpin: do [#binoutcomes]


Implemented `#binoutcomes` as a small shared FlowCore contract in `src/agent-doc` commit `14befac3` (`feat: add binary workflow outcome contract`). The new `flow::outcome` module defines `BinaryOutcomeClass` with the exact binary classes `ok`, `recoverable`, `blocked`, and `operator`, plus `BinaryOutcome` carrying a contract version, stable invariant id, proof marker, and one field-safe `next_action` token.

The contract is intentionally pure and reusable by route, queue-continuation, closeout, editor-write, and session-check code without changing those hot paths yet. It rejects empty, whitespace-bearing, or delimiter-bearing fields, so ambiguous multi-action strings like `retry,clear` cannot pass as a single decision outcome. It also serializes classes in stable snake_case and can render parseable ops-log fields: `binary_outcome=... invariant=... proof_marker=... next_action=...`.

Changed paths: `src/agent-doc/agent-doc-orchestration/src/flow/outcome.rs`, `src/agent-doc/agent-doc-orchestration/src/flow/mod.rs`, the parent `src/agent-doc` gitlink, and this session document. Verification: `tsift rewrite --run 'cargo test -p agent-doc-orchestration binary_outcome'` passed (3 tests), `make check` passed (clippy, 4568 nextest tests, doctests, and closeout SimWorld), and `gh run list --workflow CI --limit 1` shows latest CI on `main` completed successfully. Confidence: high. Escalation: none.

### Re: do [#binstalepause] — gpt-5

> **Queue prompt:**
>
> :round_pushpin: do [#binconflictscope]


> **Queue prompt:**
>
> :round_pushpin: do [#binstalepause]


Implemented `#binstalepause` in `src/agent-doc` commit `6e79ef03` (`feat: emit binary outcome for stale queue pauses`). The existing `#jbrestale` path already detected legacy and marker-backed `queue_paused` stale-supervisor churn, lifted the stale pause, restarted the supervisor once, and retried dispatch. This slice converts that recovery classification into a typed `BinaryOutcome` proof.

The controller now builds a `StaleQueuePauseRecovery` object with `binary_outcome=recoverable`, `invariant=stale_queue_pause`, `proof_marker=supervisor_restart_redirect`, and `next_action=restart_supervisor_once_and_retry`. The dispatch bail keeps the old restart marker for compatibility, but ops.log now has a durable typed proof line. The route retry path also carries the same typed fields in `route_dispatch_recovery`, both for `result=restarted` and `result=reexec_failed`, so stale-pause recovery is distinguishable from deliberate operator/spent-preset pauses.

Changed paths: `src/agent-doc/agent-doc-orchestration/src/project_controller.rs`, `src/agent-doc/agent-doc-orchestration/src/project_controller/rpc.rs`, `src/agent-doc/agent-doc-orchestration/src/route/authoritative_actor.rs`, the parent `src/agent-doc` gitlink, and this session document. Verification: `tsift rewrite --run 'cargo test -p agent-doc-orchestration dispatch_error_supervisor_restart_redirect_classification'` passed, `tsift rewrite --run 'cargo test -p agent-doc-orchestration dispatch_queue_paused_stale_supervisor_emits_restart_redirect_marker'` passed, `make check` passed (clippy, 4568 nextest tests, doctests, and closeout SimWorld), and latest CI on `main` was green via `gh run list --workflow CI --limit 1`. Install was not run because the submodule contains unrelated pre-existing dirty files; installing from that tree would promote unrelated work into the local binary. Confidence: high. Escalation: none.

### Re: do [#binconflictscope] — gpt-5

Implemented `#binconflictscope` in `src/agent-doc` commit `a447beb1` (`feat: scope binary conflicts by managed component`). The turn-scope contract now names all managed conflict surfaces: exchange, queue, backlog, status, review, done/archive, gitlink, and editor writeback. Queue remains node-scoped through the driver address, so unrelated sibling queue edits stay independent, while managed output surfaces are component-scoped and same-component incompatible edits classify as turn-affecting contention.

Changed paths: `src/agent-doc/agent-doc-core/src/turn_scope.rs`, `src/agent-doc/agent-doc-orchestration/src/preflight/semantic_diff.rs`, `src/agent-doc/specs/02-document-format.md`, the parent `src/agent-doc` gitlink, and this session document. Verification: `tsift rewrite --run 'cargo test -p agent-doc-core turn_scope'` passed, `tsift rewrite --run 'cargo test -p agent-doc-orchestration derive_turn_scope_resolves_queue_driver_and_sets'` passed, `tsift rewrite --run 'cargo test -p agent-doc-orchestration scoped_drift_gate'` passed, `make check` passed (clippy, 4571 nextest tests, doctests, and closeout SimWorld), and latest CI was green via `gh run list --workflow CI --limit 1`. Install was not run because the submodule still contains unrelated pre-existing dirty files; installing from that tree would promote unrelated work into the local binary. Confidence: high. Escalation: none.

### Re: do [#binconflictdedupe] — gpt-5

> **Queue prompt:**
>
> :round_pushpin: do [#binconflictdedupe]


Implemented `#binconflictdedupe` in `src/agent-doc` commit `5c3f7f99` (`feat: dedupe tracked workflow symptoms`). Backlog/review additions can now carry a `[symptom-key invariant=<id> document=<doc-id> component=<component> content_hash=<hash>]` marker; repeated symptoms with the same key attach an indented `evidence:` continuation to the existing open/gated item instead of inserting another tracked backlog/review row. The attachment is logged as `symptom_dedupe_attached ...` with invariant/document/component/content_hash fields, and same-cycle-added ids now record only real insertions.

Changed paths: `src/agent-doc/agent-doc-core/src/pending.rs`, `src/agent-doc/agent-doc-orchestration/src/pending_cmd.rs`, `src/agent-doc/agent-doc-orchestration/src/write.rs`, `src/agent-doc/specs/02-document-format.md`, `src/agent-doc/tests/pending_integration.rs`, the parent `src/agent-doc` gitlink, and this session document. Verification: `tsift rewrite --run 'cargo test -p agent-doc-core op_add_dedupes_symptom_key_by_attaching_evidence'`, `tsift rewrite --run 'cargo test --test pending_integration write_pending_add_dedupes_symptom_key_into_existing_backlog_item'`, `tsift rewrite --run 'cargo test --test pending_integration write_review_add_dedupes_symptom_key_into_existing_review_item'`, `tsift rewrite --run 'cargo test -p agent-doc-core pending'`, `tsift rewrite --run 'cargo test --test pending_integration'`, `make check` (clippy, 4574 nextest tests, doctests, closeout SimWorld), and `gh run list --workflow CI --limit 1` all passed/green. Install was not run because the submodule still contains unrelated pre-existing dirty files; installing from that tree would promote unrelated work into the local binary. Confidence: high. Escalation: none.

### Re: do [#qflood2] — gpt-5

> **Queue prompt:**
>
> do [#qflood2]


Resolved `#qflood2` as an existing landed implementation plus verification pass, with no new source edits in this cycle. The route/controller fix is already in `src/agent-doc` commit `9bc62c6f` (`fix(route): report benign in-flight dispatch coalesce as deduped-success not exit-1 (#qflood2)`): the controller’s benign duplicate-dispatch bail carries `failed_stage=coalesced_in_flight`, route classifies that marker across the IPC wrapping, returns the already-running pane as a deduped-success result, and logs `route_dispatch_deduped ... reason=in_flight_coalesce` instead of surfacing an exit-1. The SimWorld proof landed in `7bfc82e3`, and the between-turn `/clear` settle/composer-trigger dedupe proof landed in `fe4cf0d5`.

Verification run this cycle: `cargo test -p agent-doc-orchestration qflood2_coalesce_marker_survives_ipc_wrapping`, `cargo test -p agent-doc-orchestration qflood_coalesces_busy_in_flight_redispatch_and_releases_on_ready`, `cargo test -p agent-doc qflood2_in_flight_coalesce_is_deduped_success_not_a_dispatch_failure`, `cargo test -p agent-doc qflood2_drain_dedups_trigger_already_pending_in_composer`, `cargo test -p agent-doc qflood2_drain_holds_trigger_until_own_clear_settles`, and `gh run list --workflow CI --limit 1` all passed/green. Changed paths this cycle: session document only via `--done qflood2`; the unrelated dirty submodule files were left unstaged. No JB plugin update is needed specifically for `#qflood2`; live operator testing should use a freshly installed/recycled agent-doc binary/cdylib, but I did not run install here because the submodule has unrelated dirty source changes and installing from that tree would promote unrelated work. Confidence: high. Escalation: none.
<!-- agent:boundary:a7cc1694 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
---
- ~~do [#recyclerestart-verify]: This is automatable. You are misattributing it to operator driven.~~
- ~~do [#rtwbcast-verify]~~
- ~~do [#c2b6]~~
- ~~do [#1j8q]~~
- ~~do [#anw0verify]~~
- ~~I have a good feel for when agent-doc behaves correctly. Can we lock down behavior invariants that the agent can use to determine if there are workflow issues...so we can automate bug detection and fixes?~~
- ~~Please add and prioritize backlog + queue items to implement recommendations from "binary logic for stale queue pauses and conflicts" response.~~
- ~~:round_pushpin: do [#binoutcomes]~~
- ~~:round_pushpin: do [#binstalepause]~~
- ~~:round_pushpin: do [#binconflictscope]~~
- ~~:round_pushpin: do [#binconflictdedupe]~~
- ~~do [#qflood2]~~
- do [#qt32]
- do [#fcc0-live-verify]
- do [#archstatekernel]
- do [#archproofledger]
- do [#archcomponentmerge]
- do [#archsimworldmatrix]
- do [#archuistates]
- do [#cwsp]
- do [#wfinvcatalog]
- do [#wfinvdoctor]
- do [#wfinvautofix]
<!-- /agent:queue -->
###
<!--
Please make backlog replacements for all gated review items. We need to complete the plan, not have any more gates after this round. Let me know when I need to install JB plugin updates. Make sure all JB live gate items have adequate logging to prove/disprove the item. Clear the review items. I will test, files bugs, and you will check the logs.
---
`'/clear' is disabled while a task is in progress
---~~~~
A recent monsterrodholders.md session had much churn after I edited the queue while a turn was running. One churn item included deleting a queue item that I added.
---
agent-doc reentrant run on equityfundingsource.md had recovery churn.
```
● The recovery is complete and fully consistent. The binary converged the now-drained queue (empty block, queue_active:
  false), no_changes: true, both responses are committed in the session doc, and both code deliverables are on
  origin/main. The auto-loop terminated naturally.

  Let me record the specific recovery path for this failure mode, since it's a recurring dogfooding pattern
```
---
- ~~This session's console stdout is bleeding in front of the opencode tui. ![img_16.png](img_16.png)~~
---
The last run had a code fence stripped away by agent-doc.
#spec-test-build-install-commit-push
---
What are #next-steps to fix bugs?
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#archstatekernel] Implement a pure typed workflow state kernel for agent-doc route/queue/closeout/editor-write decisions: input evidence -> decision enum -> allowed mutation -> required proof. Move stale-supervisor, queue-drainability, captured-response, and live-buffer decisions behind this boundary before adding more route-edge guards.
- [ ] [#archproofledger] Add an append-only operation proof ledger for queue heads, response captures, patch writes, actor generations, and terminal proofs. Every consumed/deferred/retried/superseded head should have one durable proof row keyed by operation id and content hash.
- [ ] [#archcomponentmerge] Define and enforce managed-component merge semantics: queue, exchange, backlog, status, review, and done archive each have separate bases and conflict rules; only same-component incompatible edits become user-visible conflicts.
- [ ] [#archsimworldmatrix] Add SimWorld/property transition-table coverage for the architecture primitives: stale supervisor recycle, queue edit during closeout, degraded/file-IPC fallback, live buffer drift, compact/replay, and operator-only/no-drainable queue outcomes.
- [ ] [#archuistates] Replace raw route/write conflict strings in JB/Codex/OpenCode surfaces with a small typed outcome vocabulary: queued behind owner, recovered and retried, deferred for operator proof, no drainable work, real component conflict, and blocked with exact unblocker.
- [ ] [#cwsp] [operator-verify] [recommended] If queue editing still interrupts an active Codex owner after installing `src/agent-doc` 2464ba38 and recycling the live supervisor onto that build, add a busy-owner queue-edit suppression guard: queue edits during an active cycle must stay as document state until current closeout commits, must not dispatch into the busy owner pane, and must not let a pre-finalize assistant response be reclassified as a user prompt.
- [ ] [#qt32] [operator-verify] #lvbremain Operator live zero-drift PROOF of the 08b supervisor-in-process cutover (cannot be agent-done — needs a live editor). Restart a session, drive one edit-during-finalize cycle, agent reads ops.log for write_authority action=routed + plugin_watch_readonly + supervisor_host_gate hosting=in-process + ZERO drift lines. [recommended]
- [ ] [#fcc0-live-verify] [operator-verify] #fcc0 live JB 0-dialog proof: with the current agent-doc binary/cdylib installed and the route-owned supervisor recycled onto it, drive a live JetBrains queue/template/compact write path and confirm ops.log shows editor IPC convergence (for example queue_consume_writeback ... transport=editor_ipc) and zero JetBrains File Cache Conflict dialogs. Cannot be agent-done; requires live IDE observation.
- [ ] [#wfinvcatalog] Create a machine-readable workflow invariant catalog: each invariant has id, fact sources, ok predicate, disproof markers, severity, safe remediation, operator-gated remediation, and SimWorld/regression coverage. Start with queue continuation, stale supervisor, closeout commit, editor convergence, generation redirect, and parent gitlink invariants.
- [ ] [#wfinvdoctor] Add an agent-doc workflow doctor/diagnose command that evaluates the invariant catalog against preflight/session-check JSON, ops.log markers, cycle_state, actor state, git state, and editor proof sidecars; output typed ok/recoverable/blocked/operator results with exact repair commands.
- [ ] [#wfinvautofix] Add an invariant-driven autofix planner that executes only proven safe repairs automatically, gates destructive/operator actions, de-duplicates repeated symptoms by invariant id, and records proof markers so queue/backlog items do not multiply for the same root cause.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#rtwbcast-verify] [operator-verify] rtwbcast rung 4 final proof remains gated: code is landed in agent-doc cddad87b and headless targeted-delivery proof exists via SimWorld, but agent closeout on 2026-06-16 found only JetBrains live broadcast queue markers and no JetBrains-plus-VS Code live convergence/ack proof. Acceptance still requires opening the same managed doc in JB and VS Code, editing both buffers, and confirming both converge with no clobber and zero File Cache Conflict. Plan: tasks/agent-doc/plan-realtime-editor-watcher.md
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
