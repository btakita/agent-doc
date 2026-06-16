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

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260616-043202.md`*

Compacted content:
❯ - Archived 11 response topic(s): tmux submit profile and OpenCode Enter; OpenCode tmux Enter; lazily-rs for submit-key policy; 8 more
❯ - Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac...

### Re: JB Run Agent Doc still does not work — gpt-5

> **Queue prompt:**
>
> :pushpin: JB `Run Agent Doc` still does not work. What is wrong with the codebase? This is disturbing? Seems like we need a refactoring project to be able to reliably make this fix.


The immediate `captured response baseline no longer matches current document` repro is not an Enter-key or JetBrains-plugin problem. The live state now shows the installed CLI, controller, and route-owned supervisor all fresh by inode, and `src/agent-doc` is clean/even with `origin/main`. The earlier queue-edit deadlock was fixed and pushed as `0d403254` (`fix: replay captured responses across queue-only drift`); latest `src/agent-doc` CI is green (`27594082265`).

The part that is still wrong is architectural: closeout recovery is split across `route`, `repair`, `capture::validate_replay`, `session-check`, snapshot/CRDT sidecars, editor IPC convergence, and controller queue state. That makes each fix a local exception around a failure string. So yes, a refactoring project is warranted, but the right target is narrow: make closeout recovery a typed state machine, then make `JB Run Agent Doc` consume that typed decision instead of parsing low-level errors.

I added the plan: `tasks/agent-doc/plan-run-agent-doc-closeout-recovery-state-machine.md`. It proposes a single `CloseoutRecoveryDecision` boundary covering replay, stale-capture retirement, sidecar reset from visible markdown, queue-behind-active-closeout, already-committed, and blocked outcomes. The acceptance bar is that route no longer string-matches `captured response baseline...`; it handles a typed recovery result with SimWorld coverage for queue edits, write-applied fragmentation, compaction, stale snapshots, stale ack-content, and JB dispatch during open closeout.

Lean is not the first practical fix. It could help later for a small model of queue/cycle/capture invariants, but it will not stop poor architecture by itself. The immediate fix is to move the Rust code to exhaustive enums, one evidence-gathering API, and property/SimWorld tests so recovery paths cannot bypass the policy.

Proof for this cycle:
- Changed paths: `tasks/agent-doc/plan-run-agent-doc-closeout-recovery-state-machine.md` plus this session document/backlog update.
- Commands: `agent-doc --version`; `agent-doc admin inspect ... --json`; `agent-doc controller status --project-root ... --ensure`; `gh run list --workflow CI --limit 1`; `git status --short --branch`; `git diff --check --no-index /dev/null tasks/agent-doc/plan-run-agent-doc-closeout-recovery-state-machine.md`; `agent-doc reset --from-current --preserve-session ...`; `agent-doc cancel ...`; `agent-doc patch ... exchange`; `agent-doc queue consume ...`; `agent-doc write --commit ...`.
- Verification: whitespace checks passed; latest `src/agent-doc` CI is green; no Rust build/test/install was run because this cycle added a plan and backlog item, not source code.
- Confidence: high on diagnosis; medium on sufficiency until the refactor lands.
- Escalation: none.

### Re: #jbruncloseoutstate — gpt-5

> **Queue prompt:**
>
> Would lean-lang help to create proper invariants? So agents can create unnecessary architecture sprawl?


> **Queue prompt:**
>
> :pushpin: [#jbruncloseoutstate]


Implemented the first concrete closeout-recovery state-machine slice in `src/agent-doc` and marked `#jbruncloseoutstate` done.

The fix is deliberately narrow: route now classifies a blocked active closeout through a typed `RouteCloseoutBlockDecision` instead of falling straight through to the raw capture/replay error. Prompt-bearing JB reroutes still enqueue the prompt behind the active closeout; plain `Run Agent Doc` / active-queue reroutes now detect the existing live queue head and wait behind the closeout instead of surfacing `captured response baseline no longer matches current document` as a terminal operator error. If there is neither a prompt to preserve nor an active queue head, it still fails closed with the original blocker.

This reuses the existing `flow::closeout` recovery state machine rather than adding a parallel architecture. Lean is still not the right first tool here: the useful invariant is now expressed in Rust as an exhaustive route decision with regression tests, and the broader formal-model question can come after the typed recovery boundary is stable.

Proof for this cycle:
- Changed paths: `src/agent-doc/agent-doc-orchestration/src/route.rs`, parent submodule pointer `src/agent-doc`, and this session document.
- Code commit: `src/agent-doc` `2464ba38` (`fix(route): wait active queue behind blocked closeout`).
- Commands: `cargo fmt`; `cargo test -p agent-doc-orchestration closeout_block_decision -- --nocapture`; `cargo test -p agent-doc-orchestration route::tests::drain_reaps_completed_review_item_across_all_surfaces -- --nocapture`; `make check`; `gh run list --workflow CI --limit 1`; `git diff --check`.
- Verification: focused route tests passed; `make check` passed clippy, 4,550 nextest tests, doctests, and the ignored medium SimWorld closeout corpus. Latest GitHub CI on `main` is green (`27594082265`).
- Confidence: high for this JB Run Agent Doc failure mode; medium for the full family until more route/repair/session-check decisions are collapsed into the shared closeout recovery layer.
- Escalation: none.

### Re: How can agents think architecturally instead of sprawl code? — gpt-5

> **Queue prompt:**
>
> How can we have the agents think architecturally instead of the default sprawl code?


You get architectural behavior by making architecture a required artifact of the task, not by asking the agent to be more thoughtful in prose.

The practical pattern is:

1. Name the invariant before code changes. For this class: "route, repair, session-check, and write may not each invent their own closeout recovery policy." If the invariant cannot be stated, the agent is not ready to edit.
2. Name the owner boundary. One module or state machine owns the decision; adapters call it. Local call sites may gather facts or apply effects, but they do not decide policy independently.
3. Force a transition table. For closeout recovery, every input shape must land in one of: replay, retire stale capture, reset sidecars, queue behind active closeout, already committed, or blocked. A missing row is a bug.
4. Make new abstractions pay rent. The rule should be: no new helper/module unless it deletes duplicated policy, makes an impossible state unrepresentable, or gives tests a stable seam to hit. Otherwise it is sprawl.
5. Require negative design notes. The agent should say what it is deliberately not abstracting and why. This catches "architecture theater" early.
6. Put the architecture in tests. Exhaustive enum matches, SimWorld/property tests, and small pure decision tests are what keep the design from collapsing back into string-matched exceptions.
7. Treat Lean/TLA as a second-stage tool. It can model a small stable kernel once the Rust boundary exists. It will not rescue a codebase where the policy boundary is still implicit.

So the agent prompt should not be "think architecturally." It should be closer to: "Before editing, identify the invariant, the policy owner, the transition table, the minimal touched surfaces, and the tests that prove no local exception bypasses the owner. If you cannot do that, stop with a plan instead of adding code." That turns architecture from taste into a checkable contract.

Proof for this cycle:
- Changed paths: this session document only.
- Commands: `agent-doc preflight tasks/agent-doc/agent-doc-bugs2.md`; `agent-doc plan tasks/agent-doc/agent-doc-bugs2.md`.
- Verification: document response only; no code changed in this cycle.
- Confidence: high.
- Escalation: none.

### Re: typing into the queue during a turn interrupted closeout — gpt-5

> **Queue prompt:**
>
> Typing into the queue during the turn cause the conversation to be interrupted. ![img_30.png](img_30.png). A pre-finalize response was uncommitted causing the response to be treated as a user prompt ![img_31.png](img_31.png). After restart, JB `Run Agent Doc` error.
>
> ```
> [route] target tmux session: 0
> Error: authoritative actor generation 438 for tasks/agent-doc/agent-doc-bugs2.md owns pane %110 but route could not drain the active closeout before dispatch: captured response baseline no longer matches current document for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md. Rebuild sidecars without clearing session state: `agent-doc reset --from-current --preserve-session /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md`
> ```


This report has two distinct failure modes.

The after-restart JB `Run Agent Doc` error is the one fixed by `src/agent-doc` commit `2464ba38`: when route cannot drain an active closeout, it now makes a typed decision. If the reroute has a prompt, it queues that prompt behind the closeout; if there is already a live queue head, it waits behind that head instead of surfacing the raw `captured response baseline no longer matches current document` reset command. That covers the error block pasted in this queue item.

The screenshot that says `Conversation interrupted` is separate. That is the current owner conversation being interrupted while a queue edit happens during an active turn. The route fix prevents the later recovery failure, but it does not by itself guarantee that a queue edit cannot wake/interrupt the running Codex turn. If that still reproduces after installing `2464ba38`, it needs its own busy-owner guard: queue edits during an active cycle must remain document state only until the current closeout commits; they must not dispatch into the busy owner pane and must not turn a pre-finalize assistant response into a user prompt.

Important live-state note: preflight warns the installed binary and cdylib are stale relative to `2464ba38`, so JetBrains/tmux will not show the route fix until `make install` is run in `src/agent-doc` and the live supervisor/session is restarted or recycled onto the new build.

Proof for this cycle:
- Changed paths: this session document only.
- Commands: `agent-doc preflight tasks/agent-doc/agent-doc-bugs2.md`; `agent-doc plan tasks/agent-doc/agent-doc-bugs2.md`; inspected `img_30.png` and `img_31.png`.
- Verification: document diagnosis only; no new code changed in this cycle. The referenced route fix remains verified by the prior `make check` run.
- Confidence: high that the pasted route error is covered by `2464ba38`; medium on the interruption part until tested on an installed build.
- Escalation: none.

### Re: architecture-first runbook — gpt-5

> **Queue prompt:**
>
> :pushpin: Create a skill/runbook to apply the architecture strategy that you recommended.


Created the architecture-first runbook for agent-doc implementation turns.

The canonical guidance is in `.claude/skills/agent-doc/runbooks/architecture-first.md`, with a top-level pointer at `runbooks/architecture-first.md` and an `AGENTS.md` runbook listing update so Codex/OpenCode prompt reuse can discover it. The runbook turns the earlier architecture strategy into a required contract: invariant, policy owner, transition table, evidence inputs, allowed edit surfaces, verification, and explicit out-of-scope work before broad code changes.

The durable rule is now concrete: if an agent cannot name the invariant or policy owner, it should stop with a plan instead of adding another local exception. For closeout-recovery work, the runbook also calls out typed state-machine decisions, edge-localized error text, SimWorld/property coverage, and Lean/TLA only as a second-stage validation tool after the Rust boundary is stable.

No new backlog item is needed from this response; the requested runbook now exists and is committed.

Proof for this cycle:
- Changed paths: `.claude/skills/agent-doc/runbooks/architecture-first.md`, `runbooks/architecture-first.md`, `AGENTS.md`, and this session document.
- Repo commit: `b606d87e3` (`docs(agent-doc): add architecture-first runbook`).
- Commands: `agent-doc preflight ...`; `agent-doc plan ...`; `sed`/`rg` inspections of skill and runbook surfaces; `git diff --check`; `make check`; `gh run list --workflow CI --limit 1`; `gh workflow list`; `gh run list --workflow "Agent Doc" --limit 1`; `gh run view 27595064029 --json ...`; `gh run view 27595064029 --log-failed`; `git commit -m "docs(agent-doc): add architecture-first runbook"`.
- Verification: `git diff --check` passed; `make check` passed, including instruction-files tests and the `src/agent-doc` check suite. The root repo has no workflow named `CI`; the latest `Agent Doc` workflow run (`27595064029`) is red on pushed head `227e708`, but both jobs had empty steps and `--log-failed` returned `log not found`, so I treated that as an external CI-start failure rather than a failure from this local runbook commit.
- Confidence: high for the runbook coverage and discoverability.
- Escalation: none.

### Re: architecture-first should be dogfooding-only — gpt-5

> **Queue prompt:**
>
> The architecture first runbook should only apply to agent-doc development & dogfooding...It should not be installed.


Corrected. The architecture-first guidance is now only a repository-level agent-doc development/dogfooding runbook, not part of the installable agent-doc skill.

I removed `.claude/skills/agent-doc/runbooks/architecture-first.md`, made `runbooks/architecture-first.md` the self-contained canonical copy, and updated `AGENTS.md` to label it `architecture-first (agent-doc development/dogfooding only)`. I also verified there are no remaining `architecture-first` references under `.claude/skills/agent-doc`, so future skill installs will not carry this runbook.

No new backlog item is needed from this correction.

Proof for this cycle:
- Changed paths: removed `.claude/skills/agent-doc/runbooks/architecture-first.md`; updated `runbooks/architecture-first.md`, `AGENTS.md`, and this session document.
- Repo commit: `2a6f8cc78` (`docs(agent-doc): keep architecture runbook local`).
- Commands: `agent-doc preflight ...`; `agent-doc plan ...`; `rg -n "architecture-first" .claude/skills/agent-doc runbooks/architecture-first.md AGENTS.md`; `git diff --check`; `make check`; `gh run list --workflow "Agent Doc" --limit 1`; `git commit -m "docs(agent-doc): keep architecture runbook local"`.
- Verification: `git diff --check` passed; `make check` passed again. The latest `Agent Doc` Actions run is still the unrelated empty-log failure on pushed head `227e708`, not this local commit.
- Confidence: high.
- Escalation: none.

### Re: typing into the queue during a turn interrupted closeout — gpt-5

This report is two failure modes, and the logs keep them separate.

The pasted after-restart `Run Agent Doc` route error is covered by the source fix in `src/agent-doc` commit `2464ba38`: route now makes a typed blocked-closeout decision and can wait behind an existing active queue head instead of surfacing the raw `captured response baseline no longer matches current document` reset message as the operator-facing failure.

The live interruption while typing into the queue is not proven fixed from logs. Current admin state shows the controller and route-owned supervisor are fresh only relative to the installed binary inode, while preflight still warns the installed binary/cdylib predate source commit `2464ba38`. The ops log also has no `route_dispatch_drain_closeout_wait_existing_queue` marker yet, which is the marker I would expect only after the new route path is actually live.

What the logs do prove for this report:
- The old qchurn/stale-supervisor pause existed and was later resumed with `queue_resumed ... stale-qchurn-pause-cleared-after-controller-and-supervisor-match-installed-binary`.
- The interrupted/IPC corruption class is real: `ipc_snapshot_adoption_blocked ... agent-doc-bugs2.md ... reason=live_prompt_drift_after_preflight` and `ipcfullprompt_corruption_suspected ... deleted=11` appeared at `1781584793`.
- Later queue writebacks for this document used `transport=editor_ipc`, so editor IPC was participating in closeout/queue convergence.
- Operator-verify heads are still being skipped as `reason=operator_verify session=live_ipc`; that is not completion proof.

So I did not mark the queue-edit interruption fully fixed. The source-side route failure is fixed, but the live behavior still needs an installed `2464ba38` binary/cdylib and a deliberate repro/verification pass. No new backlog item is needed here because the existing `#cwsp` follow-up captures the remaining busy-owner queue-edit suppression work if the interruption still reproduces after installation.

Proof for this cycle:
- Changed paths: this session document only.
- Commands: `agent-doc preflight ...`; `agent-doc plan ...`; `agent-doc admin inspect tasks/agent-doc/agent-doc-bugs2.md --json`; `rg --files .agent-doc | rg 'ops|log|sidecar|state'`; `rg -n "route_dispatch_drain_closeout_wait_existing_queue|ipcfullprompt_corruption_suspected|ipc_snapshot_adoption_blocked|queue_consume_writeback|go_queue_skip_undrainable|stale-qchurn|queue_resumed" .agent-doc`; `rg -n "route_dispatch_drain_closeout_wait_existing_queue" .agent-doc/logs/ops.log`; `rg -n "dispatch_stale_generation_redirect|dispatch_retry_after_stale_generation" .agent-doc/logs/ops.log`.
- Verification: log inspection only; no repo code changed in this cycle. The relevant source fix remains `2464ba38`, but live install proof is still missing.
- Confidence: high on the split diagnosis; medium on the live interruption until tested after installing the source fix.
- Escalation: none.

### Re: #cwsp — gpt-5

I did not implement `#cwsp` as code in this cycle because its precondition is not satisfied yet.

`#cwsp` is conditional: add a busy-owner queue-edit suppression guard **if** queue editing still interrupts an active Codex owner **after** installing `src/agent-doc` `2464ba38` and recycling the live supervisor onto that build. Current preflight still warns the installed binary/cdylib predate `2464ba38`, and the log check found no `route_dispatch_drain_closeout_wait_existing_queue` marker. That means there is no valid post-install repro to implement against yet.

I reclassified `#cwsp` as `[operator-verify]` so go-mode will not keep draining it as ordinary implementation work. The unblock is: install/recycle `2464ba38`, reproduce typing into the queue during an active Codex owner turn, and only if it still interrupts, implement the suppression guard described by `#cwsp`.

Proof for this cycle:
- Changed paths: this session document/backlog only.
- Commands: `agent-doc preflight ...`; `agent-doc plan ...`; `agent-doc read ... --component queue`; `agent-doc read ... --component backlog`.
- Verification: state inspection only; no code changed. The blocker is the stale live install plus missing post-install repro marker.
- Confidence: high that `#cwsp` should be operator-verify gated rather than implemented now.
- Escalation: none.

### Re: current state after queue-edit interruption and JB route error — gpt-5

The screenshots and current controller state point to a real closeout-interruption class, but the specific pasted `Run Agent Doc` route error is stale relative to the live state now.

`img_30.png` shows Codex hitting `Conversation interrupted` after a stale route-owned-supervisor / IPC-drift recovery path, then exiting with a `codex resume ...` command. `img_31.png` shows the kind of pre-finalize proof body that had been produced but not committed, which is exactly the dangerous state where the response can be mistaken for live prompt drift after restart.

What changed since that generation-438 error: `src/agent-doc` is clean at `2464ba38` (`fix(route): wait active queue behind blocked closeout`), `agent-doc --version` reports `0.34.1`, and `agent-doc admin inspect` now shows the controller and route-owned supervisor both fresh against the installed binary inode. The same inspect output reports this queue head as `state: consumed`, and later dispatch attempts moved past the earlier `queue_paused`/generation-438 failure. So I would not treat the pasted `captured response baseline no longer matches current document` message as the current active blocker.

What is still not proven: I found no `route_dispatch_drain_closeout_wait_existing_queue` marker in `ops.log`, and the live interruption behavior has not been re-tested on this now-fresh controller/supervisor state. That means the source-side route failure is fixed, but the broader "typing into the queue while the owner is active must not interrupt Codex or strand a pre-finalize response" invariant is still only conditionally covered by `#cwsp`.

I am leaving `#cwsp` at the top as `[operator-verify]`: with the fresh installed build, the next useful proof is a live queue edit during an active Codex owner turn. If it still interrupts or strands the response, `#cwsp` becomes the implementation item for the busy-owner suppression guard; if it does not, we can close that item with log proof instead of adding another local exception.

Proof for this cycle:
- Changed paths: this session document only.
- Commands: `tsift status`; `agent-doc --version`; `agent-doc preflight ...`; `agent-doc plan ...`; `agent-doc read ...`; `git status --short --branch`; `git -C src/agent-doc status --short --branch`; `git -C src/agent-doc log --oneline -5`; `find . -name img_30.png -o -name img_31.png`; `agent-doc admin inspect ... --json`; targeted `rg` checks in `.agent-doc/logs/ops.log`; inspected `img_30.png` and `img_31.png`.
- Verification: document/log/screenshot diagnosis only; no source code changed, so I did not run the full build/test suite.
- Confidence: high that the generation-438 JB route error is stale after the current fresh controller/supervisor state; medium on the queue-edit interruption until the fresh-build live repro is run.
- Escalation: none.
- Confidence: high that the generation-438 JB route error is stale after the current fresh controller/supervisor state; medium on the queue-edit interruption until the fresh-build live repro is run.
- Verification: document/log/screenshot diagnosis only; no source code changed, so I did not run the full build/test suite.
- Commands: `tsift status`; `agent-doc --version`; `agent-doc preflight ...`; `agent-doc plan ...`; `agent-doc read ...`; `git status --short --branch`; `git -C src/agent-doc status --short --branch`; `git -C src/agent-doc log --oneline -5`; `find . -name img_30.png -o -name img_31.png`; `agent-doc admin inspect ... --json`; targeted `rg` checks in `.agent-doc/logs/ops.log`; inspected `img_30.png` and `img_31.png`.
- Changed paths: this session document only.
- Escalation: none.
- Confidence: high that the latest JB route attempt submitted and was consumed; medium on why the operator-facing status still reads as failure until we capture the exact current JetBrains UI/status text after a `route_success` attempt.
- Verification: log/controller/pane diagnosis only. No source code changed, so I did not run the build/test suite.
- Commands: `tsift status`; `agent-doc --version`; `agent-doc preflight ...`; `agent-doc plan ...`; `agent-doc admin inspect ... --json`; `ls -t .agent-doc/state/editor-route-attempts`; `sed -n '82,100p' .agent-doc/state/editor-route-attempts/tasks__agent-doc__agent-doc-bugs2.md-b9c648a4.txt`; targeted `rg` checks in `.agent-doc/logs/ops.log`; `tmux capture-pane -p -t %3 -S -120`; `ls -l .agent-doc/state/editor-route-errors`; `find .agent-doc/state/editor-route-errors -maxdepth 1 -type f -print`.
- Changed paths: this session document only.
- Not a stale controller/supervisor: `agent-doc admin inspect` reports both controller and route-owned supervisor match the installed binary inode.
- Not the old `queue_paused` qchurn failure.
- Not the generation-438 `captured response baseline no longer matches current document` route failure.
- `2026-06-16T05:38:32Z` attempt `1781588312473-2`: route started, then `route_success` after 3.884s.
- `2026-06-16T05:34:54Z` attempt `1781588094485-1`: route started, then `route_success` after 5.176s.
Handled the agent-doc cycle for [agent-doc-bugs2.md](/home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md:226).

The response was written and committed as `a9bb1ac84` (`agent-doc(agent-doc-bugs2): 2026-06-16 01:37:00`). `session-check` passed for the committed cycle. I did not run the full build/test suite because this cycle only added document/log/screenshot diagnosis, not source changes.

Key outcome: the pasted generation-438 JB route error is stale relative to the current fresh controller/supervisor state, but the broader queue-edit interruption still needs live verification under `#cwsp`. Queue continuation is false, with only stale/noise queue lines left, so I did not auto-loop.

### Re: JB Run Agent Doc still does not submit; usage map — gpt-5

> **Queue prompt:**
>
> :pushpin: JB `Run Agent Doc` still does not submit. Find all locations where this is used.


I searched the canonical `src/agent-doc` submodule for `Run Agent Doc`, `RunAgentDoc`, `AgentDoc.Submit`, `SubmitAction`, and the `route --dispatch-only --plain-trigger` path. I ignored the separate `src/agent-doc-verify-*` worktrees because they are verification copies and would duplicate stale history.

Runtime locations:

- `editors/jetbrains/src/main/resources/META-INF/plugin.xml:36` registers action id `AgentDoc.Submit`, text `Run Agent Doc`, shortcut `ctrl shift alt A`, and menu placements.
- `editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/SubmitAction.kt:25` is the JetBrains action implementation. It resolves the project/file, coalesces rapid re-fires, starts a durable attempt ledger, waits for typing idle, saves all documents, calls `TerminalUtil.sendToTerminal(...)`, then registers the file with `PromptPoller`.
- `editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/RunAgentDocAttemptLedger.kt:9` owns the durable per-click attempt file under `.agent-doc/state/editor-route-attempts`, including stages like `clicked`, `typing_idle`, `route_start`, `route_success`, and `route_failed`.
- `editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/TerminalUtil.kt:245` is the plugin route runner. It builds the command, spawns the process, retries only startup-actor failures, classifies busy/queued/persistent failures, clears stale route errors on success, and writes the final attempt stage.
- `editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/TerminalUtil.kt:402` builds the actual command: `agent-doc route --dispatch-only --plain-trigger --debounce 0 --wait-for-ready 60 <relativePath>`.
- `editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/TerminalUtil.kt:1141` classifies route output into queued, busy/running, retryable-starting, or persistent failure.
- `editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/TerminalUtil.kt:1445` and `:1450` build the visible still-running / queued messages.
- `editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/TerminalUtil.kt:1489` persists route failures under `.agent-doc/state/editor-route-errors`; `:1510` clears that file on success.
- `src/main.rs:486` defines the `agent-doc route` CLI and its `--dispatch-only`, `--plain-trigger`, `--debounce`, and `--wait-for-ready` flags. `src/main.rs:2181` wires those flags into `agent_doc_orchestration::route::run(...)`.
- `agent-doc-orchestration/src/route.rs:1800` parses frontmatter/harness config and applies the plain-trigger override; `:1822` chooses managed vs dispatch-only routing; `:1855` clears editor route errors on successful route.
- `agent-doc-orchestration/src/route.rs:1971` through `:2085` drains or classifies an open closeout before route dispatch, which is where the old baseline-mismatch class lived.
- `agent-doc-orchestration/src/route.rs:3026` through `:3084` handles blocked closeout decisions: enqueue prompt behind the closeout or wait behind an existing queue head.
- `agent-doc-orchestration/src/route/dispatch_only.rs:50` is the dispatch-only reopen path used by JB `Run Agent Doc`. It waits for ready panes, refuses protected/busy terminal states, queues active-turn reroutes into `agent:queue`, and then submits or checks proof.
- `agent-doc-orchestration/src/route/dispatch.rs:821` sends the routed reopen and waits for dispatch-start proof; `:866` logs `route_submit_observation ... dispatch_start_proven`, and `:922` logs accepted-without-proof failures.
- `agent-doc-orchestration/src/sessions.rs:333` and `:483` define the shared tmux submit helper: literal text followed by named tmux `Enter` for Codex/Claude/OpenCode/default harnesses.
- `agent-doc-orchestration/src/project_controller/rpc.rs:332` through `:370` owns dispatch coalescing semantics, with explicit operator dispatches such as JB `Run Agent Doc` allowed through while auto re-fires can be deduped.

Specs/tests that govern this behavior:

- `editors/jetbrains/SPEC.md:58` through `:66` is the main JetBrains `Run Agent Doc` contract: typing-idle wait, route command, attempt ledger, retry behavior, queued/still-running UX, accepted-only proof failure, and stale error-file clearing.
- `editors/jetbrains/AGENTS.md:45` says repeated `Run Agent Doc` should supersede stale plugin-spawned route processes.
- `editors/jetbrains/src/test/kotlin/com/github/btakita/agentdoc/SubmitActionTest.kt` checks the action ordering: ledger, typing-idle wait, save, route.
- `editors/jetbrains/src/test/kotlin/com/github/btakita/agentdoc/RunAgentDocAttemptLedgerTest.kt` checks attempt file naming and stale attempt suppression.
- `editors/jetbrains/src/test/kotlin/com/github/btakita/agentdoc/TerminalUtilTest.kt` checks the command flags, retry classification, busy/still-running messages, queued-pending classification, and accepted-proof behavior.
- `specs/07-session-tmux-commands.md:48` through `:57` is the tmux/session contract for dispatch-only `Run Agent Doc` delivery and proof.

Live evidence still says the latest JB route submitted: the two newest attempt-ledger entries for this file ended at `route_success`, and `ops.log` has `route_submit_observation ... phase=dispatch_start_proof result=dispatch_start_proven ... proof=consumed` for the latest one. There is no current `.agent-doc/state/editor-route-errors/tasks__agent-doc__agent-doc-bugs2.md.txt` file. If the UI still reads as “did not submit,” the next code target is likely the plugin/status presentation around `TerminalUtil`/attempt-ledger/admin projection, not the tmux submit primitive itself.

Proof for this cycle:
- Changed paths: this session document only.
- Commands: `agent-doc preflight ...`; `agent-doc plan ...`; `rg` searches for `Run Agent Doc`, `RunAgentDoc`, `AgentDoc.Submit`, `SubmitAction`, `dispatch-only`, and `plain-trigger`; targeted `sed`/`nl -ba` reads of `plugin.xml`, `SubmitAction.kt`, `RunAgentDocAttemptLedger.kt`, `TerminalUtil.kt`, `src/main.rs`, `route.rs`, `route/dispatch_only.rs`, `route/dispatch.rs`, `sessions.rs`, and `project_controller/rpc.rs`; `git status --short --branch`; `git log --oneline -3 -- tasks/agent-doc/agent-doc-bugs2.md`.
- Verification: code-navigation and log diagnosis only; no source code changed, so I did not run the build/test suite.
- Confidence: high on the location map; high that latest route submitted/was consumed; medium on the remaining operator-facing “still does not submit” symptom until the exact current JB UI/status text is captured after a `route_success` attempt.
- Escalation: none.
<!-- agent:boundary:add525a6 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- ~~:pushpin: JB `Run Agent Doc` still does not submit. Find all locations where this is used.~~
---
:pushpin: After restarting a session in the terminal, JB `Run Agent Doc` errors. It should not error.
```
resync: pruned 3 stale session(s)
[route] target tmux session: 0
Error: project controller command `dispatch` failed: dispatch blocked for tasks/agent-doc/agent-doc-bugs2.md: failed_stage=queue_paused reason=#qchurn no-op churn: go-mode re-injecting :pushpin: [#qchurn] each idle boundary; all 16 heads are clean-session/operator-verify and undrainable under stale host supervisor pid 2715614; zero drainable work. Pausing to stop the flood until restart onto fresh binary. receipt_id=3649 blocked_head_bytes=132 blocked_head_sha256=ae121dad65e5b00a6c504db706985aee4e8d6ed00988ba4244f51196952739d7 trigger_bytes=44 trigger_sha256=a55a6572fd4dc952bc476bd516d3a7801b1e63409715f063b4b7351ea112ade5
```
---
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
- [ ] [#cwsp] [operator-verify] [recommended] If queue editing still interrupts an active Codex owner after installing `src/agent-doc` 2464ba38 and recycling the live supervisor onto that build, add a busy-owner queue-edit suppression guard: queue edits during an active cycle must stay as document state until current closeout commits, must not dispatch into the busy owner pane, and must not let a pre-finalize assistant response be reclassified as a user prompt.
- [ ] [#lazilyqrestore] [operator-verify] Restore/account for lost queue content in `tasks/software/lazily-rs.md`: that document is foreign-owned by another pane, so capture the intended lost queue heads from the owning lazily-rs session/doc history/snapshots/sidecars, restore them as backlog items there, or record explicit user-removal proof. Related #queue-clear-unrun-items/#manual-queue-head-loss/#queue-user-edit-overwrite.
- [ ] [#recyclerestart-verify] [operator-verify] #recyclerestart live proof + promotion: Q1 (recycle ENOENT / restart) + Q2 (FFI self-heal sync guard, supersede-newer-generation) are committed (agent-doc 0b31e90c, fa6a230a) but NOT promoted onto the live stale supervisor — the img_23 stall is the pre-fix binary. Recycle/restart the supervisor onto the new build (agent-doc admin recycle --all-projects, or restart), then confirm: kill-pane + JB Sync Tmux Layout no longer reports Sync deferred, and a real idle/next-queue recycle restarts/re-clears the agent session. Cannot be agent-done. Plan: tasks/agent-doc/plan-pcp-recycle-no-restart-sync-block.md
- [ ] [#rtwbcast-verify] [operator-verify] rtwbcast rung 4 final proof — two-live-IDE CRDT broadcast convergence: with the installed build, edit the same managed doc in JB and VS Code at once and confirm both editor buffers receive the broadcast patch and converge (no clobber). Implementation landed agent-doc cddad87b (id/deliver/merge, SimWorld-covered, CI green). Cannot be agent-done — needs two live editors. Plan: tasks/agent-doc/plan-realtime-editor-watcher.md
- [ ] [#c2b6] [operator-verify] compactdrift live JB Compact Exchange re-verify: with a freshly make-install-ed agent-doc (installed cdylib currently predates aafa6510), run JB Compact Exchange on a doc whose HEAD holds a finalized Re: response and confirm it compacts WITHOUT the "refusing to auto-adopt committed historical response patchback / out-of-band typed_component_drift" bail. Any persistent failure from a NON-wedged session files against stuckhandoff2, not here. Code complete + committed agent-doc bca26cfd; 108 commit_ tests green this cycle.
- [ ] [#1j8q] [operator-verify] #suprecyclelock live proof: drive a real next_queue_item supervisor self-recycle (go-mode queue drain over a freshly-installed binary) while a sibling/concurrent launcher holds the shared project-root controller-launch.lock; confirm the execve'd start no longer surfaces "controller launch already in progress ... (os error 11)" on the pane and ops.log shows the contended waiter adopting the published controller instead. Fixed in agent-doc-orchestration project_controller.rs LaunchLock::acquire_blocking + connect_or_launch.
- [ ] [#anw0verify] [operator-verify] #anw0 redirect live proof: install the new agent-doc binary (cargo build --release + cargo install + lib-install) then drive a real recycle / restart-supervisor race so a dispatcher holding a superseded generation hits the new path. Confirm ops.log shows dispatch_stale_generation_redirect prior_generation=N next_generation=N+1 then dispatch_retry_after_stale_generation and a clean re-dispatch (no terminal generation-N-is-closed). Code+SimWorld landed agent-doc 21a70cdd / superproject 412355fa3. Cannot be agent-done: needs a live editor + real recycle.
- [ ] [#qflood2] [operator-verify] qflood regression: JB Run Agent Doc on efs coalesced as exit-1 (dual-drain owner, gen 66 already in flight). Needs live multi-process verify; if it still collides, coalesce should report deduped-success not exit-1. Plan: tasks/agent-doc/plan-queue-dispatch-flood.md
- [ ] [#qt32] [operator-verify] #lvbremain Operator live zero-drift PROOF of the 08b supervisor-in-process cutover (cannot be agent-done — needs a live editor). Restart a session, drive one edit-during-finalize cycle, agent reads ops.log for write_authority action=routed + plugin_watch_readonly + supervisor_host_gate hosting=in-process + ZERO drift lines. [recommended]
- [ ] [#fcc0-live-verify] [operator-verify] #fcc0 live JB 0-dialog proof: with the current agent-doc binary/cdylib installed and the route-owned supervisor recycled onto it, drive a live JetBrains queue/template/compact write path and confirm ops.log shows editor IPC convergence (for example queue_consume_writeback ... transport=editor_ipc) and zero JetBrains File Cache Conflict dialogs. Cannot be agent-done; requires live IDE observation.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
