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
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->
Added prioritized #next-steps follow-up items from Codex, Claude, and agent-doc logs. Top backlog item: #nextstale.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260523-131213.md`*

Compacted content:
- Archived 1 response topic(s): queue auto, clear, and repair cleanup
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ Read the Codex, Claude, agent-doc logs for #next-steps to fix agent-doc bugs.
### Re: #next-steps log review — gpt-5

I reviewed the relevant Codex, Claude, and agent-doc logs for `#next-steps` failures and compared them against the completed archive so I did not re-add already-closed work.

Already represented in completed/reaped work: stale top-backlog reporting (`#nexttop`), prompt-follow-up drift classification (`#followdrift`), file-IPC live edit ack proof (`#liveipc` / `#ipcackreg`), starting actor timeout loops (`#startloop`), graph contract validation (`#agbr`, `#gxct`, `#aggtp`), and prompt-prefix loss/normalization families (`#spfxnorm`, `#bppfxstrip2`, `#lastpfx`, repair/redelivery items).

New backlog items added, in priority order:

1. `#nextstale` — ops.log shows a stale preflight-started cycle abandoned a prompt containing `#next-steps`; add deterministic coverage proving that recovery still surfaces the prompt as actionable work.
2. `#nextrunmiss` — Codex history reports an embedded natural-language `#next-steps` request that was not run; cover preset expansion and routing when the token is inside a domain-specific request.
3. `#nextswallow` — Claude history reports `#next-steps` after compact being committed before evaluation and then producing a no-change response; add a compact-follow-up regression for prompt-bearing text swallowed by commit recovery.
4. `#nextorder2` — history includes reversed backlog ordering from `#next-steps`; add end-to-end preset expansion coverage that preserves priority/declaration order and prints the correct top id.

Top of backlog: `#nextstale`.

Commands: `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, `rg` over Codex/Claude/agent-doc logs, targeted `sed`/`rg` checks against the done archive and source tests. Verification: no code changed; backlog mutation was applied through the binary-owned pending-add path before this finalize retry committed it. Escalation: none. Confidence: medium-high; the new items are grounded in current log lines, while several adjacent issues were intentionally left closed because they already exist in the completed archive.
<!-- agent:boundary:e2693d62 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#nextstale] [recommended] Add deterministic coverage and recovery proof for stale preflight-started cycles whose abandoned prompt contains #next-steps; current ops.log shows repair_preflight_stale_prompt_cycle_abandoned for the prompt 'Left/Right buttons still do not work with agent-doc opencode. #next-steps', followed by post_commit_user_follow_up, so the next run must prove the prompt is still surfaced as an actionable prompt_target rather than silently absorbed.
- [ ] [#nextrunmiss] [recommended] Reproduce and fix embedded natural-language #next-steps dispatch misses; Codex history reports 'Why did you not run "Please analyze failed orders and bot traffic on monsterrodholders.com. #next-steps"', so preset expansion and route/plan classification need coverage when #next-steps appears inside a domain request instead of as a standalone line.
- [ ] [#nextswallow] [recommended] Add a compact-follow-up regression for Claude history where '#next-steps' after compact was committed and the next run said 'No changes detected ... Nothing to respond to'; verify preflight step-2 safe-status/OOB commit recovery cannot hide prompt-bearing compact follow-up text.
- [ ] [#nextorder2] [recommended] Add an end-to-end #next-steps backlog-order fixture that expands prompt-presets into multiple pending additions and asserts the live backlog preserves priority/declaration order, with the printed top backlog id matching the first inserted item.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
