---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent_doc_format: template
agent_doc_write: crdt
agent: codex
claude_model: claude-opus-4-6
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260514-204344.md`*

Compacted content:
- Archived 10 response topic(s): #orchmidrun; orchestration follow-up; sync orchestra premature stop; 7 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ Review the logs to find issues with agent-doc. #next-steps
### Re: log review — gpt-5

I reviewed the current session log (`.agent-doc/logs/b26b9957-8de6-4eae-bb98-5d7fe4d6b781.log`), the shared ops log, and today's debug log. The most actionable issues are now captured in the backlog.

1. Empty orchestrate closeouts are recurring after successful skill writes. The ops log shows `write_origin ... origin=orchestrate`, then `zero_patches_warning`, then `late_fallback_patch_rejected ... reason=already_committed` on the same document, for example around `.agent-doc/logs/ops.log:114891` and `.agent-doc/logs/ops.log:115050`. That looks like orchestration is still trying to persist an empty/malformed response after another path has already committed the real response.

2. Partial response checkpoints continue after the owning cycle is already stale or superseded. In the same window, checkpoints for older cycle IDs keep landing after later commit success events, for example `.agent-doc/logs/ops.log:114887` through `.agent-doc/logs/ops.log:114890` and then another stale-lock recovery at `.agent-doc/logs/ops.log:114908`. This is probably why `repair_preflight_stale_lock` is so noisy in this session log.

3. `document_dirty_after_commit` is over-broad as a reap reason. The logs show successful commits followed by `route_owned_reap_decision ... reason=document_dirty_after_commit`, while nearby commit staging says the remaining drift is `post_commit_user_follow_up` / `post_commit_local_drift kind=user_follow_up` (`.agent-doc/logs/ops.log:114897` through `.agent-doc/logs/ops.log:114900`). That may be behaviorally okay, but the reason should distinguish "new user prompt exists" from "assistant closeout left the doc dirty".

4. Codex managed capability proof is visibly expensive on startup. The current session records `network_child` proof times of about 15.9s and 13.4s before ready (`.agent-doc/logs/b26b9957-8de6-4eae-bb98-5d7fe4d6b781.log:5803` and `:5829`). If proof must remain fail-closed, it should at least be cached per binary/environment or moved so it does not delay pane readiness for every fresh start.

5. Passive sync is close to its budget ceiling. `controller_actor_lookup` exceeds its 250ms phase budget in recent safe-passive runs, including 321ms and 354ms (`.agent-doc/logs/ops.log:114853`, `.agent-doc/logs/ops.log:115015`). Total sync still passes, but the lookup path is the clear latency hotspot.

I did not see a fresh failure for the JB `API Error: 400 Could not process image` case or for `session_restart refused` in the latest session slice I inspected. The dispatch proof for the current reroute is healthy: it reached `proof=consumed proof_scope=dispatch_start` at `.agent-doc/logs/ops.log:115093` through `.agent-doc/logs/ops.log:115095`.

No code was changed and I did not run the test suite; this was a log triage and backlog-capture pass.
### Re: managed cycle closeout bugs — gpt-5

Implemented and verified the five queued agent-doc fixes.

- `#ds58`: template-mode orchestration now wraps clean plain child responses in an explicit `patch:exchange` block before finalize, while transcript-shaped/full-document output still fails closed.
- `#djwb`: partial-response checkpoint writers now stop once their original cycle commits, is abandoned, or is replaced, preventing stale partial ledgers from being mutated.
- `#m2hx`: route-owned liveness now reports `post_commit_user_follow_up` when the remaining dirty document content is a new prompt, while non-prompt drift still reports `document_dirty_after_commit`.
- `#ha62`: managed Codex network child proof now reuses an in-process cache keyed by harness, command, probe args, and environment, and logs cached proof as `child_dns_https_cached`.
- `#aymr`: safe-passive sync now prefers the live local actor projection before controller actor lookup and logs the skipped controller path with an immediate latency sample.

Updated specs and `VERSIONS.md` alongside focused regression coverage. I ran `cargo fmt`, `cargo test --lib`, targeted regressions for route-owned liveness and orchestrate transcript rejection, `make check`, `cargo build`, `cargo install --path . --force`, and local `make tmux-ci`; all passed. I also checked the latest GitHub `CI` workflow run, which was green on `main` and includes `make check` plus `make tmux-ci`.

Committed and pushed:

- `src/agent-doc`: `762f0adc` (`fix: tighten managed cycle closeout paths`) to `20260422/codex-agent-backend`
- parent repo: `532cba414` (`agent-doc: tighten managed cycle closeout paths`) to `main`
<!-- agent:boundary:e7625b3e -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue auto -->
preset spec-test-build-install-commit-push
- do [#ds58]
- do [#djwb]
- do [#m2hx]
- do [#ha62]
- do [#aymr]
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
