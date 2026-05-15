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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260515-191700.md`*

Compacted content:
- Archived 1 response topic(s): #kw0r process-global test isolation
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ agent-doc autocompacted root.md when running on claude. agent-doc should not autocompact unless there is an explicit setting in the frontmatter or config.toml

❯ spec-test-build-install-commit-push
### Re: Claude auto-compaction guard — gpt-5

Implemented the Claude auto-update compaction guard.

Changed `src/agent-doc` so rendered Claude/shared instruction surfaces now default to `agent-doc skill install --harness claude --reload restart`; `--reload compact` plus `/compact` prompting is reserved for sessions that explicitly opt into `agent_doc_auto_compact` in document frontmatter or project `.agent-doc/config.toml`. I also added project config parsing for `agent_doc_auto_compact` with the legacy `auto_compact` alias, updated the frontmatter comment, refreshed specs/runbooks/README/DEV/VERSIONS, and reinstalled the local Codex instruction mirrors.

Verification passed:
- `cargo test skill::tests::`
- `cargo test project_config::tests::`
- `make check`
- `make tmux-ci` (228 live/ignored tmux tests passed)
- `make build`
- `make install`
- `agent-doc --version` -> `agent-doc 0.33.16`
- `agent-doc skill install --harness codex`

CI review: latest parent `Agent Doc` run `25936973260` had `check` and `tmux-ci` jobs fail before any steps were created; `gh run view --log-failed` returned `log not found`, so I’m treating that as an external CI-start blocker rather than a code/tmux regression. Local `make check` and `make tmux-ci` are green.

Commits prepared before this document closeout:
- `src/agent-doc`: `4e99330` (`fix: gate claude auto-update compaction`)
- parent repo: `a2bf1795d` (`agent-doc: gate claude auto-update compaction`)

Per the session-document closeout order, I will push those commits after this finalize step completes.
<!-- agent:boundary:f6d17a9b -->
<!-- /agent:exchange -->
###
<!--
Use TEST_THREADS=8. spec-test-build-install-commit-push
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
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
