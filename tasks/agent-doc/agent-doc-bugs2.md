---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: claude
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
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

## Status

 <!-- agent:status patch=replace -->

completed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260611-172026.md`*

Compacted content:
- Archived 6 response topic(s): #vbc1 next agent-doc bug backlog; #queueeditloss; #eqrecovery; 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

### Re: #next-steps to fix bugs before release — opus-4-8

Backlog was empty; I rebuilt it in priority order. **Top of backlog: `#2v7b` (#jbcodexsubmit)** — the JB→Codex submit bug you just reported (diagnosed below), the only functional release blocker.

Priority order:

1. **`#2v7b` #jbcodexsubmit** — JB `Run Agent Doc` doesn't submit to Codex. Functional, agent-fixable, has a plan. *Release blocker.*
2. **`#qt32` #lvbremain** *(recommended)* — operator live zero-drift proof of the 08b supervisor-in-process cutover. Code-complete + pushed already; only a live editor can prove it. Operator-drive, not agent-completable.
3. **`#02bw` #fmtchurn** — the working tree has 13 files of uncommitted **rustfmt-only** whitespace churn (no semantic changes — I checked the diff). Decide commit-as-`style:` vs discard before tagging. Cosmetic.

Per memory the entire 08b cutover (pcpc1–pcpc5, removal rung) is code-complete and on `origin/main`; no implementation backlog remains there. So the path to release is: land `#2v7b`, you live-verify it + `#qt32`, clean up `#fmtchurn`, tag.

### Re: JB `Run Agent Doc` on Codex still doesn't submit the prompt — opus-4-8

> **Queue prompt:**
>
> What are #next-steps to fix bugs? I want to release soon.


Diagnosed and filed as `#2v7b` with a plan at `tasks/agent-doc/plan-jb-codex-submit.md`.

**Root cause:** Codex isn't owned-PTY managed (only claude/opencode are), so the JB action routes through `route.rs::send_command_unchecked` → direct-pane submit, which sends the trigger text + `\r` as **one** `send-keys -H` payload. That path *already detects* the non-submit — when the trigger text is still visible after the acceptance timeout it logs `route_submit_issue ... issue=prompt_not_submitted` (`TriggerStillVisible`, route.rs:6624) — but it just returns `TimedOut`. **There is no re-submit fallback.** The Codex TUI appears to treat the merged trailing `\r` as a literal newline (drafted multi-line) rather than a submit key, which is exactly your "entered into input, I had to submit manually" symptom.

**Fix (planned, not yet implemented):** Codex-scoped one-shot fallback — when direct-pane acceptance detects `TriggerStillVisible` on a Codex pane, send a *separate* bare `Enter` key event and re-poll once, gated to `harness == codex` so claude/opencode are untouched, bounded to a single retry. Plus a `route_submit_resubmit ... action=enter_key result=accepted|still_visible` proof line so your live test is provable from `ops.log` without a live editor on my side.

I did **not** implement the fix this turn — it's a dispatch-path change that needs your live Codex+JB verification (I can't reproduce the Codex TUI submit behavior here), and your two prompts were "what's next" + a bug report, which per your own `#agent-doc-bug` convention means plan-and-backlog. Say "do `#2v7b`" (or "go") and I'll implement it, add SimWorld coverage, `make check` + build + `lib-install`, then hand it to you to live-verify the `route_submit_resubmit` proof.
<!-- agent:boundary:99011766 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- ~~What are #next-steps to fix bugs? I want to release soon.~~
- :pushpin: JB `Run Agent Doc` on Codex still did not submit the prompt to Codex. The prompt was entered into the input, but I had to manually submit it to start the queue.
<!-- /agent:queue -->
###
<!--
Please make backlog replacements for all gated review items. We need to complete the plan, not have any more gates after this round. Let me know when I need to install JB plugin updates. Make sure all JB live gate items have adequate logging to prove/disprove the item. Clear the review items. I will test, files bugs, and you will check the logs.
---
`'/clear' is disabled while a task is in progress
---
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
- This session's console stdout is bleeding in front of the opencode tui. ![img_16.png](img_16.png)
---
The last run had a code fence stripped away by agent-doc.
#spec-test-build-install-commit-push
---
What are #next-steps to fix bugs?
-->

## Backlog

<!-- agent:backlog priority queue --> 
- [ ] [#2v7b] #jbcodexsubmit JB `Run Agent Doc` on Codex enters the routed prompt but does not submit it (operator must press Enter manually). Root cause: Codex direct-pane dispatch in route.rs::send_command_unchecked detects prompt_not_submitted (TriggerStillVisible) but has no re-submit fallback; the merged text+`\r` send-keys payload is not treated as submit by the Codex TUI. Fix: Codex-scoped one-shot Enter re-submit + `route_submit_resubmit` proof logging + SimWorld coverage. Plan: tasks/agent-doc/plan-jb-codex-submit.md. Live verify is operator-drive (check ops.log for route_submit_resubmit action=enter_key result=accepted).
- [ ] [#qt32] #lvbremain Operator live zero-drift PROOF of the 08b supervisor-in-process cutover (cannot be agent-done — needs a live editor). Restart a session, drive one edit-during-finalize cycle, agent reads ops.log for write_authority action=routed + plugin_watch_readonly + supervisor_host_gate hosting=in-process + ZERO drift lines. [recommended]
- [ ] [#02bw] #fmtchurn Uncommitted rustfmt-only whitespace churn in src/agent-doc working tree (13 files, no semantic changes). Decide: commit as a standalone `style: cargo fmt` commit or discard, before tagging a release. Cosmetic, not a functional blocker.
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
