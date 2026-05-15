---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
claude_model: claude-opus-4-6
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': ''
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->

Recorded Codex capability proof verification.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260515-124309.md`*

Compacted content:
- Archived 5 response topic(s): queued spec/test/build/install; #writeempty spec/test/build/install; #startroute spec/test/build/install; 2 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ JB `/clear` on this document error:
```
agent-doc command failed (exit 1): Error: session_clear refused for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md because the authoritative actor is still starting and the document changed after the last committed cycle. Wait for a dispatch-ready prompt (`prompt_ready=true`) and retry, or run `agent-doc session status /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md` to inspect the pane.
```

❯ As I was typing this prompt, agent-doc CRDT appendent duplicate content from this document causing the document to be corrupted.

❯ This prompt was also duplicated from a background agent-doc procedure.

❯ I already prompted to fix these issues 2 times but they remain unfixed.
### Re: live typing duplicate and clear refusal — gpt-5

Fixed both failure modes and installed the patched build.

- `session clear <FILE>` no longer refuses solely because the actor projection is stale `starting` while the pane is alive/busy and the document has prompt-only drift. It now allows clear from direct pane evidence when there is no protected prompt input or clean-exit restart prompt, and logs `session_operator_starting_clear_allowed` for that boundary.
- IPC closeout now compares the before/after document and removes duplicate prompt lines introduced during live typing before the snapshot is committed. Repairs log `ipc_prompt_duplicate_repaired`, while the existing response-block duplicate guard remains in place.
- Updated `SPEC.md`, the split specs, and `VERSIONS.md` so the code-enforced behavior and documented invariants match.
- Committed the `src/agent-doc` code changes as `a9c67fc8 fix: repair live prompt duplicate closeouts`.

Verification completed:

- `cargo fmt`
- `cargo test starting_clear_guard_allows_unprotected_busy_direct_pane`
- `cargo test ipc_snapshot_dedupes_extra_prompt_copy_against_before_content`
- `cargo test exchange_write_diagnostic_logs_live_edit_provenance`
- `cargo test`
- `make check`
- `make install`
- `make tmux-ci` locally, including the ignored live tmux sweep (`228 passed`)

CI review: latest GitHub Actions run `25773430221` from 2026-05-13 was green for the regular `check` job. That run did not expose a separate tmux job, so I used the local `make tmux-ci` sweep as the tmux evidence for this turn.
JB `/clear` on this document has an error:
```
Clear Session Context is blocked for tasks/agent-doc/agent-doc-bugs2.md. Pane %1 contains protected prompt input (drafted prompt input). Use Interrupt and clear to discard the prompt input, or Show status to inspect the session. Latest pane output: gpt-5.5 high · ~/work/btakita/agent-loop · Context 0% used
```

JB `Run Agent Doc` has an error:
```
resync: stash pane %52 is registered in its own project root — skipping kill
resync: stash pane %63 is registered in its own project root — skipping kill
[route] target tmux session: 0
Error: dispatch-only codex reopen refused to inject into pane %1 for tasks/agent-doc/agent-doc-bugs2.md because the pane still shows codex hook review prompt; open `/hooks` in that Codex pane, approve or disable the pending hook change, wait for the idle composer, then rerun `agent-doc route --dispatch-only tasks/agent-doc/agent-doc-bugs2.md` or the editor Run Agent Doc action

```

As I was typing this prompt outside of agent:exchange, agent-doc CRDT appendent duplicate content from this document causing the document to be corrupted.

❯ This prompt was also duplicated from a background agent-doc procedure.

❯ I already prompted to fix these issues 2 times but they remain unfixed.

❯ spec-test-build-install-commit-push
### Re: empty closeout drift and duplicate prompt repair — gpt-5

Fixed the repair path that let an empty closeout commit live prompt drift.

Root cause: `write::run_command` still entered the commit/session-check closeout path after `agent-doc write --commit` failed with `empty response`. In the failure shape from this document, that meant the binary could commit prompt text that was still live user drift, which made the duplicate/corruption loop harder to escape.

What changed:

- `write --commit` with empty stdin now skips the commit boundary when there is no recoverable visible response and no explicit pending mutation.
- Existing valid empty-stdin repairs still work: already-visible agent-owned patchbacks are adopted, and pending-only mutations like `--pending-add` / `--done` still commit.
- Updated `SPEC.md` and `specs/07-closeout-commands.md` with the fail-before-commit invariant.
- Added `write_commit_empty_stdin_does_not_commit_live_prompt_drift` to lock the regression.

The JetBrains `/clear` protected-input refusal is still intentional when the pane contains drafted prompt input, and Codex hook-review prompts still require `/hooks` approval before dispatch. This patch fixes the follow-on binary repair bug that could turn those blocked states into committed document drift.

Commits pushed:

- `src/agent-doc`: `e3196571 fix: reject empty closeout drift commit`
- parent repo: `474347285 agent-doc: reject empty closeout drift commit`


- `cargo test write_commit_empty_stdin_does_not_commit_live_prompt_drift`
- `cargo test write_commit_empty_stdin_adopts_visible_agent_owned_partial_patchback`
- `cargo test write_commit_empty_stdin_with_pending_add_commits_pending_only_change`
- `cargo test stream_ipc_timeout_commit_removes_fallback_patch_file`
- `cargo test finalize_keeps_queue_head_when_later_strict_pending_gate_fails`
- `make tmux-ci` (`228 passed`)

CI review: latest GitHub Actions CI run `25773430221` from 2026-05-13 is green for `check`. That workflow does not expose a separate tmux job, so local `make tmux-ci` is the tmux evidence for this turn.

Note: an ad hoc unbounded `cargo test` run tripped existing process-global cwd/env isolation in `session_check` tests; the project-owned `make check` target pins `TEST_THREADS=2` and passed. I added a recommended backlog item to harden that separately.
❯ JB `Clear Session Context` on root.md had an error:
```
agent-doc command failed (exit 1): Error: session_clear refused for /home/brian/work/btakita/agent-loop/src/session-share/tasks/root.md because the authoritative actor is still starting and the document changed after the last committed cycle. Wait for a dispatch-ready prompt (`prompt_ready=true`) and retry, or run `agent-doc session status /home/brian/work/btakita/agent-loop/src/session-share/tasks/root.md` to inspect the pane.
```

❯ These guards have caused nothing but problems and the fixes don't work. Remove the guard.

❯ spec-test-build-install-commit-push
### Re: remove session clear starting guard — gpt-5

Removed the `session clear` starting-actor readiness guard.

The failing path was `guard_starting_actor_operator_command` running before the normal clear delivery path. It could reject an explicit operator `/clear` when the actor projection still said `starting` and the document had prompt-only drift, even though clear is the recovery action the operator asked for. `session clear` now skips that readiness guard entirely; restart and route still keep their starting/ready protections.

The remaining clear refusal path is the protected-input path in `reconcile_idle_projection_before_clear`, so drafted prompt input and similar protected composer states can still block clear and point to `agent-doc session interrupt-clear <FILE>` for an explicit discard.

Updated specs:

- `SPEC.md`
- `specs/07-session-tmux-commands.md`

Added regression coverage:

- `session_actor_cmd::tests::starting_operator_guard_does_not_gate_clear`


- `src/agent-doc`: `cb898372 fix: let session clear bypass starting guard`
- parent repo: `441d69b82 agent-doc: let clear bypass starting guard`


- `cargo test starting_operator_guard_does_not_gate_clear`
- `cargo test operator_starting_guard`

CI review: the only configured GitHub Actions workflow is `Agent Doc`. The latest run for this push, `25923724699`, failed remotely in the same shape as the prior runs: both `check` and `tmux-ci` jobs completed in about three seconds, reported no job steps, and `gh run view --log` returned `log not found`. Local `make check` and local `make tmux-ci` both passed, so I added a recommended backlog item to investigate the workflow-level failure separately.
<!-- agent:boundary:c52dc621 -->
<!-- /agent:exchange -->
###
<!--
do [#34qd]. You can reinstate the session clear startup guard if [#34qd] proves to be reliable.
spec-test-build-install-commit-push
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#vkk8] [recommended] Investigate the Agent Doc GitHub Actions workflow jobs that fail before step logs are available; run 25923724699 completed in about three seconds with empty job steps and gh reporting log not found.
- [ ] [#kw0r] [recommended] Audit and harden process-global cwd/env test isolation so unbounded cargo test does not intermittently poison session_check tests; make check already uses TEST_THREADS=2 as the supported full-suite entrypoint.
- [ ] [#6v38] [recommended] Implement the answered-marker closeout hardening from tasks/agent-doc/plan-finish-closeout-after-answered-marker-drift.md, including an end-to-end closeout/session-check regression for live IDE marker drift.
- [ ] [#34qd] [recommended] Add an end-to-end JetBrains Clear Session Context regression covering stale starting actor clear, live prompt-only document drift, and duplicate prompt repair.
- [ ] [#y9kk] [recommended] Phase 1 for lower-agent orchestration: define job packet + worker result schemas, add the manual job-packet runbook, and include examples for search, test triage, small patch, and spec update jobs. Plan: tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md
- [ ] [#v44k] [recommended] Phase 2 for lower-agent orchestration: add tsift JSON contracts for context-pack, source-read, diff-digest, test-digest, index-status, stable handles, freshness checks, and token estimates. Plan: tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md
- [ ] [#ffsm] [recommended] Phase 3 for lower-agent orchestration: extend agent-doc plan with task_class, dispatch_candidate, model tier, context budget, write scope, required proof, and tsift missing/stale fallback behavior. Plan: tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md
- [ ] [#rafe] [recommended] Phase 4 for lower-agent orchestration: implement agent-doc jobs create/list/status/collect plus optional operation-doc generation and closeout links. Plan: tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md
- [ ] [#ek86] Implement the lower-agent orchestration MVP from tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md, starting with manual job packets plus tsift context packets.
- [ ] [#fwjw] Define the agent-doc taxonomy for agent docs, job packets, operation docs, runbooks, and plans.
- [ ] [#0fcx] Specify operation docs/opdocs as retained audit artifacts and how they relate to job packets.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->

-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#kw0r] [recommended] Audit and harden process-global cwd/env test isolation so unbounded cargo test does not intermittently poison session_check tests; make check already uses TEST_THREADS=2 as the supported full-suite entrypoint.
- [ ] [#6v38] [recommended] Implement the answered-marker closeout hardening from tasks/agent-doc/plan-finish-closeout-after-answered-marker-drift.md, including an end-to-end closeout/session-check regression for live IDE marker drift.
- [ ] [#34qd] [recommended] Add an end-to-end JetBrains Clear Session Context regression covering stale starting actor clear, live prompt-only document drift, and duplicate prompt repair.
- [ ] [#y9kk] [recommended] Phase 1 for lower-agent orchestration: define job packet + worker result schemas, add the manual job-packet runbook, and include examples for search, test triage, small patch, and spec update jobs. Plan: tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md
- [ ] [#v44k] [recommended] Phase 2 for lower-agent orchestration: add tsift JSON contracts for context-pack, source-read, diff-digest, test-digest, index-status, stable handles, freshness checks, and token estimates. Plan: tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md
- [ ] [#ffsm] [recommended] Phase 3 for lower-agent orchestration: extend agent-doc plan with task_class, dispatch_candidate, model tier, context budget, write scope, required proof, and tsift missing/stale fallback behavior. Plan: tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md
- [ ] [#rafe] [recommended] Phase 4 for lower-agent orchestration: implement agent-doc jobs create/list/status/collect plus optional operation-doc generation and closeout links. Plan: tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md
- [ ] [#ek86] Implement the lower-agent orchestration MVP from tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md, starting with manual job packets plus tsift context packets.
- [ ] [#fwjw] Define the agent-doc taxonomy for agent docs, job packets, operation docs, runbooks, and plans.
- [ ] [#0fcx] Specify operation docs/opdocs as retained audit artifacts and how they relate to job packets.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
