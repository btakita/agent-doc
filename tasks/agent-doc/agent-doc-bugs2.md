---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': Any follow-up items to place in the acklog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#mermaid': Include explanatory mermaid diagram(s).
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->

Live IntelliJ File Cache Conflict Cancel proof captured; JB Cancel residual review items completed.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260527-224056.md`*

Compacted content:
- Archived 2 response topic(s): queued Run Agent Doc prompt update; queued Run Agent Doc prompt update follow-up
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

### Re: auto-done signal work — gpt-5

Implemented the two requested agent-doc fixes and landed them in `src/agent-doc`.

Changed paths:
- `SPEC.md`
- `src/frontmatter.rs`
- `src/project_config.rs`
- `src/session_check.rs`
- `src/plan.rs`
- `src/write.rs`
- `src/tsift_graph.rs`
- `src/jobs.rs`

What changed:
- Added `auto_done: true` frontmatter support and `[guards] auto_done = true` project config support.
- Added inline done signal detection so prompts like `#id done`, `done #id`, `complete #id`, `resolved #id`, and `mark #id done` emit `ResolveExisting` / `--done <id>` plan mutations.
- Added opt-in bare `done` handling when `auto_done` is enabled and exactly one open review item is the contextual target.
- Added response-completion auto resolution in finalize/write closeout. When `auto_done` is enabled and the response clearly completes an open tracked item, the binary now applies the missing done mutation, records cycle-state pending-done ids, and gates backlog items through review before marking them done.
- Updated pending-done repair guidance to use `agent-doc write <file> --done <id> --pending-only --commit`.
- Fixed hyphen/underscore hash id extraction for do-target parsing.
- Fixed a full-suite test race by sharing the process-global test environment lock in `jobs.rs`.

Verification:
- `cargo test` in `src/agent-doc`: passed.
- `make check` from repo root: passed.
- `cargo build` in `src/agent-doc`: passed.
- `cargo install --path .` in `src/agent-doc`: passed; `agent-doc --version` reports `agent-doc 0.34.0`.
- `cargo fmt --check`: passed.
- `git diff --check`: passed.
- `gh run list --workflow CI --limit 1`: no workflow named `CI` exists in this repo.
- Latest `Agent Doc` post-push run `26543921559`: failed in 3s with both `check` and `tmux-ci` jobs reporting empty step lists and no logs. I classified this as an external CI-start failure, not a local code or tmux regression.

Commits pushed:
- `src/agent-doc`: `e69ddaed` (`feat: auto-resolve done signals`) pushed to `origin/20260422/codex-agent-backend`.
- superproject: `dd4fba13c` (`chore: update agent-doc submodule`) pushed to `origin/main`.

Queue note: `agent-doc orchestrate --from-queue --plan` could not use the local queue because `agent:queue` contained raw prose (`JB \`Run Agent Doc\` on this file did not dispatch.`) before the preset entry. I handled the two explicit preset targets manually in this cycle and cleared the malformed queue content below.

Confidence: high for the local implementation and tests. Escalation: none; no approvals were needed.
<!-- agent:boundary:cbad3fe5 -->
<!-- /agent:exchange -->
###
<!--
The completed queue item was not removed from the queue. #mermaid
#spec-test-build-install-commit-push
---
equityfundingsource.md had the full-document IPC duplicate corruption. Then a new full-document IPC deletion under the prompt corruption happened. Both several times while editing the prompt.
#spec-test-build-install-commit-push
---
Please review the agent-doc, claude, codex logs. What are #next-steps to fix bugs?
---
When I ran the last turn, #spec-test-build-install-commit-push was removed by agent-doc.
---
As I was typing into the comment below `/agent:exchange`, the full-document IPC corruption happened, then the duplicate line happened, then the full-document IPC corruption happened again.

In addition to diagnosing and fixing the issue, please provide a mermaid showing how the issue occurred and where in the agent-doc lifecycle the full-document IPC corruption can occur. Can we mathematically eliminate all possibility of the full-document IPC corruption from occurring?

#spec-test-build-install-commit-push
-->

<!-- agent:queue auto -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#snrun] [recommended] Implement tasks/agent-doc/plan-run-agent-doc-snappy-auto-remediation.md: make JB Run Agent Doc distinguish rescued-stash accepted-only proof misses, active-turn busy collisions, and stale starting projections; add route/plugin regressions plus live replay evidence.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
