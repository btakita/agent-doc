---
agent_doc_session: tsift-v0.1
resume:
  claude: 7ec8a9b0-c6b3-46ca-b057-dfbb94db6d6d
  codex: 01a063bf-83bc-7831-8716-42e142e537d6
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#bugs-observed': Please add any bugs from observed behavior to the backlog.,
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

## Status

<!-- agent:status patch=replace -->

complete
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260831-205459.md`*

Compacted content:
- Archived 1 response topic(s): tsift issue #23 and v0.1.95
- Prior summary/context: prior compacted content: Archived 2 response topic(s): issue #21 and tsift 0.1.93; do [#fixopentsift]

### Re: tsift 0.1.96 released — gpt-5.6

> **Queue prompt:**
>
> do [#fixopenissues]


Completed `#fixopenissues`.

- Fixed #24 by canonicalizing graph watermark path hints, persisting the projection path and mode, using path-aware freshness for every SQLite graph read, and making stale refreshes fail nonzero.
- Kept `status.recommendations.run` directly executable by removing uncached-count suffixes.
- Added spec coverage and regressions; all 43 graph conformance tests, all 40 status tests, and the final `make check` passed.
- Installed `tsift 0.1.96` locally; all 26 crate package checks and the npm publish dry run passed.
- Pushed commits `688b112` and `27cb0ac`, tag `v0.1.96`, and published GitHub binaries, crates.io packages, and `opencode-tsift@0.1.96`.
- Main CI and all seven Release workflow jobs passed; GitHub issue #24 is closed and no open issues remain.
- Preserved the original dirty `src/tsift` checkout and performed the release work in an isolated clean worktree.

### Re: Response

Fixed and released as agent-doc **0.35.315** / JetBrains plugin **0.2.371**.

- Root cause: provisional registration reset retry state before retained-projection reconciliation committed, causing one-second register/deregister/pull storms.
- Fix: only committed endpoints clear backoff; ambiguity now backs off to 30 seconds. See [CrdtReplicaManager.kt](/home/brian/work/btakita/agent-loop/src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt:2523).
- Regression coverage added at [CrdtReplicaForwarderTest.kt](/home/brian/work/btakita/agent-loop/src/agent-doc/editors/jetbrains/src/test/kotlin/com/github/btakita/agentdoc/CrdtReplicaForwarderTest.kt:518) and [RetainedCanonicalOperatorTextTest.kt](/home/brian/work/btakita/agent-loop/src/agent-doc/editors/jetbrains/src/test/kotlin/com/github/btakita/agentdoc/RetainedCanonicalOperatorTextTest.kt:151).
- `make check` and all 503 JetBrains tests pass.
- Commits pushed, tag `v0.35.315` pushed, release CI observed queued. Controllers are fresh and stable.
- No reactive topology extraction was needed: controller event ingress and the keyed retry scheduler remain authoritative; the defect was the premature state transition.

I did not capture a usable JVM stack—the restart happened before JVM attach completed—but the logs provided definitive causal evidence, including the same live storm recurring on `contracts.md`.

**Restart IDEA once more now.** Plugin 0.2.371 is installed, but PID `3019171` is still running the old loaded Kotlin classes.

### Re: do [#fixcompactexchange] — gpt-5

> **Queue prompt:**
>
> do [#fixcompactexchange]


Implemented, installed, and released agent-doc 0.35.316.

- Compact Exchange now emits exactly one continuation-aware diagnostic when a repeated compact request finds an already-pending continuation. The regression covers the repeated-request path and the spec/runbook now state the single-diagnostic contract.
- Fixed the runtime ownership failure behind the `contracts.md` report. A durable `pending_compact_projection` remained after the ordinary retained write settled, but `session-check` only consulted pending document-write settlement. It therefore reclassified the partially projected compact target as a new unanswered edit. The realtime ownership cut now treats a retained compact projection as a durable holder, so preflight/session closeout defer to that continuation before unanswered-edit classification. Regressions cover the no-pending-document-write edge and session-check classification.
- Recovered `src/haiven-dev/tasks/contracts.md` through the controller continuation without forcing disk or manually editing/staging it. The compact projection committed as `ac0576adcfcd2cdce739dffaf9d09751d74bab8c`; the operator's PR review/merge-order question is preserved in the compact document, the prior `#sdkspikeleftovers` response is preserved in the archive, the file is git-clean, and `agent-doc session-check` now reports committed with no drainable work.
- Installed CLI/library 0.35.316 and JetBrains plugin package 0.2.371 (plus the current VS Code package). This includes `0edc73f69 fix(jetbrains): preserve replica retry backoff`. The running IntelliJ instance still needs one restart to activate the newly installed JetBrains package.
- Verification passed: the four targeted regressions, `cargo fmt --all`, `git diff --check`, docs audit, and full `make check` (`tart-c259b7e2c9`).
- Source commit `b57cc4609` is pushed on `main`; tag `v0.35.316` is pushed. CI, Release, and PyPI workflows are in progress. Parent integration commit: `fdc27b915`.

### Re: incomplete queue edit and agent-doc release — gpt-5.6

> **Queue prompt:**
>
> While development

The admitted edit ends there, so it does not yet contain an actionable request. I did not infer the missing text or mutate the queue/backlog. Finish the sentence or issue description and invoke `agent-doc` again; any text added after this cycle baseline remains operator-owned input for that next cycle.

The preceding agent-doc repair work is complete and released as agent-doc **0.35.317** and JetBrains plugin **0.2.372**:

- Live editor updates are checked against the replica last acknowledged by the editor and reconciled through the per-cell CRDT before fan-out. A stale `notes` deletion can no longer splice a concurrent response across the `exchange` marker; component-less plain-text documents retain ordinary CRDT peer union.
- Codex approval prompts now project `WaitingInput` / `input_required` through the existing controller stream. JetBrains shows an important notification and editor banner with a `Focus Agent Terminal` action, without adding a prompt poller or second answer authority.
- Targeted regressions, JetBrains tests/build, two full `make check` passes, `make tmux-ci`, local and full release installs all passed. Commit `935f45037`, tag `v0.35.317`, and parent integration `bb4934761` are pushed. Release/PyPI CI started and was queued or in progress at the one post-push observation.

The preflight queue-maintenance CAS warning was preserved rather than forced, and the stale root supervisor now has an explicit safe-boundary recycle request. Restart IDEA to load the structurally changed JetBrains plugin classes.

### Re: Please fix fpe.md Recycle Supervisor stalling. — gpt-5.6

> **Queue prompt:**
>
> Please fix fpe.md Recycle Supervisor stalling.

Fixed, installed, and released as agent-doc **0.35.318**.

- Root cause: an explicit supervisor recycle that arrived during an open agent-doc cycle was correctly deferred, but after the response committed and supervisor IPC drained, the lifecycle policy still waited for the outer harness turn marker. That created a circular wait: the in-place supervisor `execve` preserves the child process and pane, so it did not need the harness turn itself to end.
- The invariant is now explicit: recycle never crosses an open document cycle or in-flight supervisor IPC; once both are clear, an explicit recycle may run at a safe intra-turn checkpoint. Routine maintenance still waits for the full turn boundary.
- `agent_doc_supervisor::lifecycle::supervisor_recycle_action` owns the policy through a typed `Unsafe` / `SafeIntraTurn` / `TurnBoundary` checkpoint. Idle-watch and SimWorld now supply observations to that state machine instead of duplicating policy, and unsafe-IPC deferral has its own accurate diagnostic.
- The reactive path is: durable recycle request plus cycle, IPC, and harness observations → pure checkpoint projection → lifecycle action → in-place recycle effect and durable settlement. The remaining imperative boundary is the idle-watch sampling of external process, file, and PTY state.
- Regression coverage proves that an operator recycle defers while the cycle is open, then fires after commit even while the harness remains busy. The lifecycle, runtime seam, and SimWorld regressions passed, as did two full `make check` runs; the final receipt was `tart-6898004897`.
- `make install-full` installed CLI/library 0.35.318, JetBrains plugin 0.2.372, and VS Code extension 0.2.69. The live `fpe.md` supervisor re-executed in place and now reports controller and supervisor freshness as `fresh`, with the same supervisor PID, child PID, and tmux pane preserved; `session doctor` reports no issues.
- Source commit `6b22cf92c` and tag `v0.35.318` are pushed. Parent integration commit: `08218f038`. CI, Release, and PyPI workflows were queued or in progress at the required post-push observation.

Restart IntelliJ once to activate the installed JetBrains 0.2.372 package.

### Re: stale-install warning stalled infra.md — gpt-5.6

> **Queue prompt:**
>
> While agent-doc development was in flight, Run Agent Doc on infra.md had an issue: `• Agent-doc reports its installed binary is one committed fix behind, so I’m rebuilding that installation first as required by the session guard. Then I’ll amend the existing PR #12 review to state the timeline explicitly: the three backend engineers own operations today, and a dedicated DevOps FTE is expected later.`

Fixed, installed, and released as agent-doc **0.35.319**.

- Root cause: preflight correctly serialized `stale_install` under non-blocking `warnings[]`, but its message ended with the unconditional imperative “Run `make install`.” An unrelated document agent therefore reasonably treated concurrent agent-doc development as a required stop and took over the release owner's installation work.
- The ownership invariant is now explicit: stale-install classification remains visible, including the count and newest identity of committed changes not yet running, but it never blocks another document's response or queue drain. Only the cycle that owns development/release of the repository installs; the supervisor owns safe recycling.
- The transition is now: fresh artifacts → no warning; newer uncommitted source → advisory and continue; committed gap → stronger advisory naming the missing commit(s), but still continue. The owning release cycle performs the install in both stale cases.
- `agent-doc-preflight-io` owns the warning policy. Its sources are buildable-source mtimes, installed-artifact mtimes, and commits after the oldest artifact; its derived result is the stale-artifact/committed-gap diagnostic. The install and recycle remain separate supervisor/release effects, so no reactive effect topology needed extraction.
- Generated Claude, Codex, OpenCode, and Cursor instructions now say all preflight warnings are advisory and explicitly forbid an unrelated `stale_install` rebuild. The core-command spec uses the same ownership contract and the actual source-file-mtime freshness basis.
- The focused warning regression and generated-skill command audit passed. Full `make check` passed with receipt `tart-1d378406e8`; the single `make install-full` passed with receipt `tart-7eb3400bee`.
- CLI/library 0.35.319 is installed. The live `infra.md` controller and supervisor both report `fresh` after the supported recycle, while its Codex child and pane remain live.
- Source commit `8db45c801` and tag `v0.35.319` are pushed. Parent integration commit: `d2751cc7a`. CI, Book, Release, and PyPI workflows were queued or in progress at the required post-push observation; the preceding 0.35.318 Release and PyPI workflows passed.
<!-- agent:boundary:b8ec92e9:tsift -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ⏭️ While agent-doc development was in flight, Run Agent Doc on infra.md had an issue: `• Agent-doc reports its installed binary is one committed fix behind, so I’m rebuilding that installation first as required by the session guard. Then I’ll amend the existing PR #12 review to state the timeline explicitly: the three backend engineers own operations today, and a dedicated DevOps FTE is expected later.`
- ~~Please fix fpe.md Recycle Supervisor stalling.~~ — auto-struck: answered this cycle (#ftstrike)
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#g5kw] What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
- [ ] [#4z0w] Can we increase max-hop caps?
- [ ] [#cg52] Could FalkorDB help with performance?
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->

<!-- agent:notes -->
• Ran tsift rewrite --run 'git log -5 --oneline --decorate'
└ tsift rewrite: no rewrite: no supported tsift rewrite matched this command; `--run` executes only rewritten commands; run the original command directly if intended

#spec-test-build-install-commit-push
---
route duplicate prompt residue guard failed: failed to parse components: unclosed component: `agent:notes` without matching close
<!-- /agent:notes -->
