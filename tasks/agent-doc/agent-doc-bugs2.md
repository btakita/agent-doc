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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260515-154511.md`*

Compacted content:
- Archived 1 response topic(s): CI start blocker
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ JB `Run Agent Doc` failed on this document. No activity or response.

❯ Still getting duplicate prompts and document corruption while I'm typing in this prompt.

❯ spec-test-build-install-commit-push
### Re: JetBrains Run Agent Doc duplicate prompt corruption — gpt-5.5

I repaired this document's malformed exchange first so preflight could run. The concrete corruption was a duplicate template scaffold inserted between two `&lt;!-- /agent:exchange --&gt;` close markers; preflight was failing before any normal response cycle could start.

I fixed the underlying paths in `src/agent-doc`:

- Rust repair now recognizes and removes the duplicate scaffold-after-exchange-close shape without swallowing live prompt edits into the snapshot.
- JetBrains `Run Agent Doc` now waits for markdown typing idle before saving and routing the file to the terminal.
- JetBrains patch application now waits for typing idle before mutating the editor document, covering both socket and file-watch patch paths.
- JetBrains plugin version is bumped to `0.2.120`, and a regression test asserts `Run Agent Doc` waits for idle before save/route.

Verification completed:

- `cargo test repair_duplicate_exchange_close_scaffold_drops_inserted_template_shell --lib`
- `cargo test recover_does_not_treat_orchestration_handoff_marker_as_missing_response -- --test-threads=2`
- `cargo test repair_completed_backlog_reap_preserves_live_prompt_outside_snapshot -- --test-threads=2`
- `cargo test --all-targets -- --test-threads=2`
- `make check`
- `cargo build --release`
- `cargo install --path .`
- `agent-doc lib-install`
- `./gradlew test buildPlugin`
- `make tmux-ci` (`228 passed` ignored live tmux tests)

The latest GitHub `CI` workflow run on `main` is green, but that workflow currently has only the `check` job and no separate tmux-test job, so the tmux evidence here is the local `make tmux-ci` run.

Installed locally:

- CLI replaced at `/home/brian/.cargo/bin/agent-doc`
- FFI library installed via `agent-doc lib-install`
- JetBrains plugin installed from `editors/jetbrains/build/distributions/agent-doc-jetbrains-0.2.120.zip` into `/home/brian/.config/JetBrains/IntelliJIdea2026.1`; restart IDEA to activate it.

Commits so far:

- `src/agent-doc`: `b588090de` (`fix: debounce jetbrains agent-doc runs`), pushed to `origin/20260422/codex-agent-backend`
- parent repo: `c33324989` (`agent-doc: debounce jetbrains runs`) records the submodule pointer locally; I will push parent `main` after this session-document finalize commit so the pointer and closeout commit travel together.
<!-- agent:boundary:28ff0afe -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#kw0r] [recommended] Audit and harden process-global cwd/env test isolation so unbounded cargo test does not intermittently poison session_check tests; make check already uses TEST_THREADS=2 as the supported full-suite entrypoint.
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
