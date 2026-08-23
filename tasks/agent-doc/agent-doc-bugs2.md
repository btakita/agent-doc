---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
resume: 01a02bfa-7c55-73b2-80ff-0db00d81d1bb
agent: codex
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260821-224408.md`*

Compacted content:
- Archived 25 response topic(s): do [#skillinstallstalemirror]; do [#skillinstallconfigpath]; do [#hookhashjsprivatefield]; 22 more
- Prior summary/context: prior compacted content: Archived 53 response topic(s): file all 3 diagnosed defects into the backlog; start with #1;...

### Re: Response

The timeout was caused by an unresolved editor/CRDT write, not document size or a dead controller.

Two events were conflated:

- The explicit 90-second timeout was the `UserPromptSubmit` preflight budget, defined in [preflight_hook.rs](/home/brian/work/btakita/agent-loop/src/agent-doc/src/preflight_hook.rs:59)—not the Stop hook itself.
- The Stop hook encountered the same unresolved state because it starts with `session-check` and captured-finalize recovery in [lib.rs](/home/brian/work/btakita/agent-loop/src/agent-doc/agent-doc-codex-stop-io/src/lib.rs:147).

For `agent-doc-bugs2.md`, the log records:

- A stale JetBrains plugin generation and repeated detached-authority refusals.
- A retained compact write that never converged.
- `PayloadAbsentFromConvergedContent`, which blocked session closeout ([ops.log](/home/brian/work/btakita/agent-loop/.agent-doc/logs/ops.log:86619)).
- Two clears and a duplicate dispatch while the actor was busy.

Thus preflight—and later Stop/session-check—waited on editor convergence until the hook runner killed it. The controller and supervisor are currently fresh and `ready`, ruling out a persistent crash.

The `monsterrodholders.md` dialog exposes a related bug: the “now in HEAD” note is printed unconditionally after the controller accepts the compact request ([rpc.rs](/home/brian/work/btakita/agent-loop/src/agent-doc/agent-doc-controller-io/src/project_controller/rpc.rs:6652)), even when delivery is merely retained with `delivery_converged=false`. JetBrains also merges stderr into command output ([TerminalUtil.kt](/home/brian/work/btakita/agent-loop/src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/TerminalUtil.kt:1374)).

Four Compact Exchange attempts were recorded, but no compact commit landed after the 18:41:06 commit. The working document is only 8 additions/2 deletions from HEAD, so there is no wholesale content loss—but the note was premature and retries accumulated retained writes. Avoid further Compact Exchange retries until the JetBrains plugin generation is refreshed and the retained state settles.

### Re: Response

Fixed and released as agent-doc 0.35.258.

The JetBrains notification you missed was recoverable from the retained controller state. The actionable text was:

```
[compact] pending: the compacted target is retained for editor delivery and is not yet committed. Do not retry Compact Exchange while this target is pending; if delivery remains pending, restart the editor so its agent-doc plugin reconnects
[compact] pending: existing continuation compact-projection-ca2dcf...-b349dce3... already owns this document (commit_requested=true)
```

The separate `[compact] note: --commit persists only the compacted document state now in HEAD...` message is informational: the compact command commits the compacted document cut, but any explanation printed afterward still requires its own agent-doc response/finalize cycle.

There were three linked Compact Exchange faults:

- Production reconnect bypassed the semantic retained-write replay path, while JetBrains could abandon a transient post-registration read.
- A newer retained Exchange target could remain behind an older converged compact summary. Recovery did not have a guarded way to replay the newer target while preserving sibling components.
- Most importantly, compact completion failed once before the editor replica existed and then became one-shot. The retained continuation stayed unchanged, and replica registration did not emit the durable projection change that the Effect was waiting for.

The fix routes reconnect through the durable semantic policy, adds fenced JetBrains retries and durable IDEA error logging, safely rebases a strictly newer retained Exchange target only when the authoritative post-boundary tail is empty, and advances a controller retry edge from changed CRDT registration/projection frontiers. Identical pull frontiers are deduplicated, and any operator text in the authoritative Exchange tail still fails closed.

The Stop-hook timeout was a separate boundedness bug: repair/recovery work could outlive the harness's 30-second Stop deadline, so the harness killed the hook before it returned. The hook now owns a 20-second internal budget inside that deadline; longer recovery remains supervisor-owned. Preflight and Stop timeouts were also projected explicitly.

Verification:

- Focused Rust and JetBrains regressions pass.
- Full `make check` passes.
- Public-name hygiene and `git diff --check` pass.
- The installed CLI/native/plugin report 0.35.258.
- The original live continuation settled as state event 21522 without issuing another compact request.
- JetBrains converged and saved hash `28652794132a52b2f34e21c495137b4f411f060f5e2fb66538d2484ee33907ff`.
- The document worktree is clean at commit `5e0af2cf`.
- Public fix commit `51ee565c9` and tag `v0.35.258` are pushed. CI and release workflows were started.
The `agent-doc` cycle was not admitted because the document authority remained `delivery_pending` for 3079 ms.

No preflight, document changes, or commit were performed. Retry after the current transition settles.

### Re: agent-doc reconciliation, compaction, pane sync, CPU, and indentation

You were right about the document acknowledgment and the indentation.

The missing acknowledgment had a specific Codex lifecycle cause: after a clean committed closeout, the Stop path deleted the exact thread-to-document binding. Your next ordinary prompt in the same Codex thread therefore looked unrelated to agent-doc, so I answered in chat without acquiring document write-back debt. Clean closeout now parks that exact-thread binding. The next ordinary same-thread prompt must cross the agent-doc write/commit boundary; explicit `/clear`, `/new`, or a new agent-doc invocation severs or replaces the binding.

I also corrected the Kotlin formatting mistakes: the focus projection fields/executor are class-indented consistently, and the remaining over-indented test assertion is aligned with its `assertEquals` arguments.

The other fixes are now released together:

- Compact Exchange compares and replaces only `agent:exchange`. Frontmatter, backlog, queue, review, and all other sibling or scratch content are irrelevant to the compact conflict decision and are rebased from current authority verbatim. Only concurrent changes inside the same Exchange cell fail closed. The retained/pending path uses the same target-scoped rule, so unrelated edits no longer create a second compact owner.
- Restarted agents wait for any retained editor-delivery projection to settle before the automatic agent-doc trigger. This prevents the post-restart `session-check` from racing reconciliation and reporting the authority/disk divergence you saw after switching `backend.md` to Claude.
- JetBrains rejected-canonical recovery now replaces the bad CRDT replica and rebuilds from the exact unchanged valid editor baseline with a generation/path-fenced bounded retry. A malformed retained canonical can no longer drive the recurring reconciliation loop.
- Automatic tmux pane sync now retains and retries a failed editor-surface publication with 100/200/400/800/1600/2000 ms capped backoff. The one-time controller-recycle `Connection refused` can no longer permanently strand auto-sync until another editor event.
- The 108% controller spike was full ordered-CRDT materialization inside `RelayHub::is_safe_to_evict` while the global relay registry lock was held. Eviction now compares the cached committed CRDT state vector. Detached test controllers under narrowly classified `.tmp*` roots also self-reap after 60 idle seconds; the accumulated temporary controllers are gone.
- The currently loaded old JetBrains plugin is still re-registering documents about every seven seconds, which explains the remaining transient 24–38% controller bursts after installation. The fixed package is installed in all four detected JetBrains IDE installations, but JetBrains requires an IDE restart to activate the new Kotlin generation. I did not restart the IDE because that could discard unsaved editor buffers.

Verification is green: `make check` passed all 8,995 workspace tests (253 skipped), doctests, documentation/release audits, and TLA+ models; the Compact Exchange crate passed all 68 tests; the JetBrains suite passed all 465 tests and built the plugin. The single parity `make install-full` completed successfully.

Released and pushed as agent-doc `v0.35.260` at `92f163fd1`; the agent-loop pointer/instruction update is `2739460ab`. GitHub Book is green; CI, Release, PyPI, Pages, and the parent Agent Doc workflow were in progress at closeout.

### Re: Agent-doc controller, Compact Exchange, restart, tmux sync, CPU, and formatting fixes — gpt-5

I fixed and released the agent-doc causes reported in this exchange.

#### Released fixes

- **Tmux layout and auto-sync:** ordinary Markdown files no longer enter the agent-document attachment path, so opening a non-agent document cannot abort Sync Tmux Layout. Focus changes on either editor split now drive the same project-scoped layout reconciliation. The live `agent-doc` window has reconciled from three panes to two: pane `%0` owns this document and pane `%11` is the backend owner.
- **Compact Exchange scope:** compaction admission and divergence checks are scoped to `agent:exchange`. Backlog, review, queue, and unrelated document changes do not block or invalidate an exchange-only compaction target.
- **Retained Compact Exchange recovery:** a retained target has a single controller-owned continuation. Reconnect, live-hub recovery, malformed retained replicas, exact replay, and clear-session settlement no longer create competing continuations or attach loops.
- **Restart Agent:** restart preserves the document-selected harness, including a switch to Claude, and routes through the already-owned pane instead of provisioning an extra pane.
- **Clear Session Context:** an unavailable editor authority no longer leaves the actor stuck at `agent-doc: awaiting response`; the clear transition settles to `ready` while attachment recovery remains controller-owned.
- **Controller timeout and CPU spike:** retained CRDT validation is linear and quarantines cyclic input before traversal. Replica updates use Lazily's unprojected merge, visible text is cached between mutations, exact replay is rejected early, commit-barrier coverage no longer constructs a throwaway full replica, and controller handoff carries the same-frontier visible projection beside the encoded state. The replacement therefore does not synchronously order the retained operation graph while a five-second registration/start RPC is waiting.
- **Document formatting:** this response uses unindented exchange markers and indentation only for content that is structurally nested. The earlier improperly indented additions were a response-generation mistake, not an intended document format.

The fixes were released as agent-doc `v0.35.261` through `v0.35.265`, with the final controller fix at commit `7756da7c3`. Lazily `v0.56.1` is commit `d45478402`, and the parent workspace records both at `2d9a5bba6`.

#### Verification

- `make check` passed.
- `make tmux-ci` passed.
- Focused CRDT merge, relay quarantine, retained-handoff, and clear-session settlement tests passed.
- The private-name denylist scan reported zero hits.
- The optimized `agent-doc 0.35.265` binary, native library, skills, and JetBrains plugin `0.2.355` were installed.
- Backend, the equity document, and this bug document all inspect as `state=ready` with `projection_lag=false`.
- The exact backend start path now reaches the controller immediately. From pane `%0` it returned in 0.027 seconds with the correct ownership guard because `%0` owns this document; it no longer reached the old five-second controller timeout.
- The sustained 100% controller loop is gone. The final five-second sample averaged 5.8% of one core during normal polling, and the post-fix profile contains no `ordered_ids` hot loop.

#### Existing retained backend delivery

I did not retry Compact Exchange, resubmit the response, force disk, or replace the queued backend edit. The final backend `session-check` completed in 5.8 seconds and still reports the pre-existing retained document-write delivery as unsettled. That is now an editor-receipt issue, not a controller timeout.

The final plugin package was installed after the current IDEA process started, and hot reload reported the running editor endpoint unavailable. Restart IDEA once more to load plugin `0.2.355`; after it reconnects, run only:

    agent-doc session-check tasks/backend.md

If it reports the delivery as applied and requests a commit, run the requested `agent-doc commit tasks/backend.md`. Do not retry Compact Exchange while that retained target owns the document.

The agent-doc release workflows were still running when checked. The parent workspace CI jobs were prevented from starting by the GitHub account billing limit, and Lazily's tag workflow could not obtain a crates.io trusted-publishing token because that repository has no trusted-publisher configuration; Lazily `0.56.1` was already published successfully through the authenticated local release path.

### Re: Compact completion, recurring queue commands, and closeout delivery — Codex

Fixed and released as agent-doc 0.35.267.

The retained Compact Exchange defect is fixed. A retained compact whose exact canonical target is already delivery-converged in the editor now requests the editor's native save, re-observes exact editor/disk equality, and only then checkpoints, commits, and marks the continuation settled. The original live backend continuation was recovered through that path and committed as `4b8603fd5f918f24c5ff4119736f43fc9c162fb0`; no force-disk or manual document rewrite was used.

The recurring free-text queue defect is also fixed. A captured response may strike an answered free-text command only while its exact closeout cycle is still open. After that cycle commits, its durable historical response can no longer strike a newly typed identical command. Consequently, a later `- deploy to staging` added after a second task remains pending for that second task; the first same-cycle occurrence is still consumed normally.

Dogfooding closeout exposed and fixed one adjacent failure in 0.35.267: a serialized CLI write that has already advanced canonical CRDT authority now awaits the exact lazy editor-delivery transition and its asynchronous native-save disk receipt within the existing bounded deadline. A prompt ACK completes the original command. If the editor advances to a different cut or never acknowledges, the original captured target remains retained and the command fails closed without recapture or a behind-the-editor disk write.

Public releases:

- `0.35.266`, source commit `dc30a6168d6a84f3ebebaeac2ba1262eb88877fd`: retained Compact completion and recurring queue ownership.
- `0.35.267`, source commit `3a71c9f47`: same-command delivery/native-save receipt settlement.
- Parent projection commit `057a965cdd`; both public source tags and parent commits are pushed.
- Installed binary: `agent-doc 0.35.267`; matching JetBrains plugin package installed in four existing IDE installations.

Invariant:

A compact cannot commit from stale disk while a live editor owns a newer exact target. A response capture cannot mutate queue text outside its own open closeout cycle. An accepted serialized write can complete only from exact canonical delivery plus native-save disk proof; authority drift preserves the original intent.

Policy owner:

Compact completion policy lives in `agent-doc-compact-io`; queue-strike capture eligibility lives in `agent-doc-state-backbone`; bounded serialized delivery and native-save proof live in `agent-doc-document-realtime-io`. The controller observes these policies and performs their effects rather than inventing duplicate eligibility rules.

Transition table:

| State | Evidence | Transition |
|---|---|---|
| Retained compact, editor target equals disk | Exact canonical/disk hash equality | Checkpoint, commit, settle |
| Retained compact, target delivered but disk stale | Live editor target, delivery convergence, native-save receipt | Re-observe equality, then checkpoint/commit/settle |
| Open exact closeout capture | Matching cycle identity and nonterminal phase | May project the answered free-text strike |
| Terminal or mismatched capture | Committed/terminal proof or different cycle | No queue strike target |
| Serialized target awaiting delivery/save | Exact delivery/disk receipt within deadline | Complete the same command |
| Serialized target missing or authority advances | Timeout or different canonical hash | Preserve original retained intent; fail closed |

Evidence inputs:

Durable compact continuation identity, canonical editor text/hash, live-editor count, delivery version and convergence, disk text/hash, native-save receipt, closeout cycle identity and phase, captured-response terminal proof, and current queue authority.

Reactive topology:

Retained continuation plus editor delivery/native-save receipts derive the compact completion effect and settled event. Closeout capture plus cycle phase and queue authority derive the active strike target and CAS effect. Serialized write receipt plus delivery/native-save observations derive same-command completion or retained refusal.

Imperative extraction audit:

Native save remains an external authority effect after visible delivery, never an initial-admission disk write. Queue eligibility moved out of controller ad hoc logic into typed state policy. The serialized caller subscribes to existing delivery state and waits only for exact proof; it does not poll by resubmitting the mutation.

Allowed edit surfaces:

`agent-doc-compact-io`, `agent-doc-controller-io`, `agent-doc-state-backbone`, `agent-doc-document-realtime-io`, their focused tests, specifications, release notes, and projected version manifests/skills.

Verification:

Focused Compact, recurring queue, exact delivery, missing-ACK, single-submit, post-settlement editor-edit, and repair-race regressions pass. The complete affected crate suite passes. Full `make check` passes with artifact `tart-e6227e7a22`, and `make install-full` completed. The live retained Compact continuation settled and its session-check passed.

The public 0.35.267 CI/Release workflows were in progress at closeout; 0.35.266 CI and Release were green. The root workflow was prevented from starting by the GitHub account billing/spending-limit gate, and the separate PyPI workflow remains subject to the external project storage quota.

Out of scope:

A global redesign of recurring queue identity, force-disk recovery, GitHub account billing administration, and PyPI storage-quota administration.

### Re: Response

Fixed and released in `agent-doc 0.35.267`.

- Recurring free-text queue items now belong only to their exact open cycle. A completed earlier `deploy to staging` can no longer strike a newly added identical item.
- Retained Compact writes now require exact editor/native-save/disk convergence before committing.
- Added regressions for both defects and the adjacent serialized-write receipt race.
- Full `make check` passed; CLI and JetBrains plugin were installed.
- Source commits `dc30a616…` and `3a71c9f4…`; session commit `56b0b70d…` is pushed.
- Agent-doc session-check passed with no remaining queue work.

I did not rewrite the already-struck item in `equityfundingsource.md`; it may need to be re-added once. Future repetitions will remain pending correctly.

Public release/CI jobs are still running. The parent repository’s jobs could not start because of GitHub’s account billing/spending-limit gate; PyPI also has an existing storage-quota issue.

Full write-up: [agent-doc-bugs2.md](/home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md)

### Re: Transition table specification — Codex

No. The consolidated transition table was present only in this session document; the public specs covered its individual invariants in prose but did not publish the exact state/evidence/transition matrix.

It is now canonical in `specs/14-realtime-workflow.md` under **Retained closeout transition table**. The table documents all six cases: retained Compact with disk equal or stale, open versus terminal/mismatched response capture, and serialized delivery/save success versus timeout or authority advance. It also states that a terminal response cannot answer an identical recurring queue command added for a later task.

A red/green contract test, `realtime_workflow_spec_documents_retained_closeout_transition_table`, pins the heading and all six rows. The focused test failed before the spec addition and passes afterward. Full `make check` passed (`tart-2e306df5ef`), and `make install-full` passed (`tart-fa9fdac8b3`). Public source commit `c21797d2b` is pushed to `main`.

### Re: Transition table specification — Codex

> **Queue prompt:** Is the transition table documented in the specs?

Yes. It is now documented in `specs/14-realtime-workflow.md` under **Retained closeout transition table**, with all six state/evidence/transition rows pinned by `realtime_workflow_spec_documents_retained_closeout_transition_table`. Public source commit `c21797d2b` is pushed; full `make check` and `make install-full` passed. The exchange is large, but I did not compact while this queue head was active.
<!-- agent:boundary:0d0ed28d:agent-doc-bugs2 -->
<!-- /agent:exchange -->
## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
<!-- /agent:queue -->

## Notes

<!-- agent:notes -->
<!-- /agent:notes -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#harnessresumemap] Implement a harness-keyed resume map with claude, codex, and opencode entries; each launch or restart must read only the active harness key, and legacy scalar resume values must migrate only into the document active harness. Add cross-harness restart and compatibility regressions.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#pypi10gbquota] [operator-verify] Request a PyPI agent-doc project-size limit increase or remove superseded releases. The 0.35.248 workflow built and attested all four wheels, then PyPI rejected upload because the project has reached its external 10 GB storage quota.
- [/] [#operatorverifylive] [operator-verify] #panewindowdriftverify Live-pane proof for #panewindowdrift (0.35.187): with a two-column agent-doc layout, move the focused document's pane into the stash window, then trigger an editor tab/focus change for that document. Expect NO select-pane onto the stashed pane (ops.log shows pane_layout_projection ... focus=focus_pane_not_co_visible:...), the structural reconcile surfaces the pane, and the retry focuses it in the agent-doc window. Unblocks on a human watching a real IDE + tmux session.
- [/] [#operatorverifyrestart] [operator-verify] Restart a running JetBrains IDE (installed plugin is 0.2.350; this item originally named 0.2.340, long superseded — the single restart in #stickymdpaneverify covers it), then move from fpe-service-topology.md in a one-pane surface to agent-doc-bugs2.md in its two-column surface and confirm the first selection restores two tmux panes without a second navigation.
- [/] [#stickymdpaneverify] [operator-verify] #stickymdpane live proof: restart a JetBrains IDE — plugin 0.2.350 is ALREADY INSTALLED in all four IDE configs, so only the restart is missing, and that one restart also satisfies the install precondition for #operatorverifyrestart, #rebootselfhealverify and #falsetyping-guard — then open a two-column split with an agent-doc document in each and switch ONE column to a source file. Expect tmux to keep both panes with that column still showing its last document, instead of collapsing to one pane. Then close that split entirely and confirm tmux DOES collapse — the fallback must be memory, not invention. Unit tests pin the pure resolution but cannot prove the live IDE wiring.
- [/] [#stalesupervisorreexecdrain] [operator-verify] One supervisor is still stranded on the pre-fix image and cannot self-rescue: PID 4069526 (`agent-doc start --resume 019fd336-7ce8-7ed1-8141-d77021a00085 src/boost-client/tasks/monsterrodholders.md`, started 2026-08-05 19:12:50 EDT) maps a DELETED binary and, by the very defect #stalereexecstarve fixes, can never reach the recycle decision to adopt 0.35.186. The fix is preventive, not retroactive -- it lives in code that process does not run. Recovery requires interrupting a live Claude child (PID 2653032), which is an operator call, not an agent-doable step: either let it retire naturally when that session ends, or `kill 4069526` and re-route the document. Verify afterwards with `readlink /proc/<newpid>/exe` showing no `(deleted)` suffix. Every other live supervisor (336290, 523099, 577794, 837794, 3268523, 3854486, 3956615) already maps the current inode.
- [/] [#rebootselfhealverify] [operator-verify] Live proof for #rebootselfheal after restarting the IDE (installed plugin is 0.2.350; this item originally named 0.2.339 — reload-lib swaps the cdylib but NOT the Kotlin package, so a restart is still required, and the restart in #stickymdpaneverify covers it). Reboot the host (or stop the controller and leave a stale .agent-doc/controller.sock behind), then run Sync Tmux Layout and Run Agent Doc from the IDE and confirm both succeed without the `pane-layout state projection publish failed ... Connection refused` notification and without a human deleting the socket. Then confirm the passive lane is still free: click between editor tabs with NO controller running and verify no controller is launched (editors/SPEC.md requires the surface-observation lane to never launch one) - grep ops.log for a controller bootstrap around the tab clicks. Both reboot states are already proven end-to-end against the installed cdylib; only the in-IDE path needs a human eyeball.
- [/] [#hvw0] [operator-verify] Rotate the CARGO_REGISTRY_TOKEN secret on github.com/btakita/skill-harness — the current one returns 403 authentication failed, so CI cannot publish to crates.io (v0.1.3 was published manually from the passage token btak/CARGO_REGISTRY_TOKEN). Needs a repo-secret write, which is operator-gated. CORRECTION 2026-08-08: the claim that every tagged release now FAILS loudly at the publish step (intended behavior after 32836ad) is UNTESTED — no tag has run since. The newest Release run is v0.1.3 from 2026-07-18 and its `Publish to crates.io` step reports success, which is exactly the false green. #hvw0publishproof carries the static check.
- [/] [#c8tb] [operator-verify] Live #jbroutebusystall proof (0b49105e1): with the supervisor socket down (route logs `route_dispatch_only_authoritative_degraded_direct_pane supervisor_health=no_socket`) and the pane mid-turn, click JB Run Agent Doc. Expect `route_start` -> `route_success` in the attempt ledger to drop from ~17s to under ~2s, with `route_existing_pane_busy_active_turn_skip_wait` in ops.log and the SAME outcome as before (`route_existing_pane_already_running`, pane focused, no duplicate injection). Unit tests pin the cue gating but cannot prove the live latency win.
- [/] [#mtds] [operator-verify] #panedraftunblocker live proof: on a Claude pane whose footer reads `⏵⏵ bypass permissions on · 1 shell · ← for agents` (the variant without `(shift+tab to cycle)`), with an EMPTY composer and no active turn, trigger JB Run Agent Doc. Expect dispatch to succeed instead of refusing with `timed_out` / `unblocker=wait_for_dispatch_ready_prompt`. Then repeat with unsent text typed in the composer and expect the new refusal naming the draft with `unblocker=submit_or_clear_pane_draft`, and NO injection over that text. Finally confirm a genuinely mid-turn pane (live spinner, bare prompt) still refuses as busy. Fixed in 17848a927. This item named binary 0.35.6 and a pending idle-boundary recycle; the installed binary is now 0.35.174 and has recycled many times, so only the live eyeball remains.
- [/] [#falsetyping-guard] #agent-doc-bug provenance-aware visible-write guard. INTEGRATED to main (merge on 5bcfc07b) + binary/cdylib installed + recycled; guard tests 7/7 green. JB install step DONE — the installed plugin is 0.2.350 in all four IDE configs, superseding the 0.2.205 zip this item named; only an IDE restart is missing, covered by #stickymdpaneverify. VS Code 0.2.38 install is UNVERIFIED — no agent-doc extension found under ~/.vscode/extensions. REMAINING (operator): restart the IDE, then live proof — type during replica churn (no wedge) + genuine unsaved edit still fails closed.
- [/] [#clearsubmitunobservedverify] [operator-verify] Live re-proof for #clearsubmitunobserved (0.35.225): reproduce the reported haiven-dev.md sequence — let the supervisor go dead (or `session restart-supervisor`), then Run Agent Doc so the flow cold-starts a replacement and immediately runs Clear Session Context on the fresh pane. Expect it to SUCCEED instead of failing with `result=submission_unobserved ... unblocker=clear_submission_unobserved`, and expect `session_clear_submit_cleared_state_probe ... result=cleared_state_proven retained_lines=<small>` in ops.log. Then the half that must not regress: on a pane holding a real conversation, break submission (e.g. leave the composer wedged) and confirm the clear still fails closed with `result=conversation_retained`. Unit tests pin the predicate against synthetic captures; only a live pane exercises the tmux scrollback capture and the real harness prompt shapes.
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
- [ ] [#exttasksources] #agent-doc-feature Add external task sou
rce adapters that project PM-tool tasks into the active document/work graph as source-backed nodes, keeping local queue/closeout authority document-owned and making remote push-back explicit and audited. PRD: tasks/agent-doc/prd-external-task-sources.md
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
