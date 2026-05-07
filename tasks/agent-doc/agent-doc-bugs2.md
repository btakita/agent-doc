---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent_doc_format: template
agent_doc_write: crdt
agent: codex
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
claude_model: claude-opus-4-6
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. #follow-up-backlog
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': #follow-up-backlog Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  "#spec-test-build-install-commit-push": update spec + tests. build + install for local testing. commit + push
  "#spec-test-commit-push": update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->
<!-- normalization -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260507-192726.md`*

Active backlog:
- [ ] [#chkptcap] Periodic checkpoint capture during response streaming — capture partial responses every 30s so mid-generation pane death does not lose work entirely.
- [ ] [#sqliterepros] [recommended] Before prototyping .agent-doc/state.db, capture 2-3 current agent-doc failures where cross-file authority drift or non-atomic state writes would have been fixed by one transactional store rather than by the single-owner actor/supervisor model alone.
- [ ] [#sqliteeval] [recommended] Review plan tasks/agent-doc/plan-sqlite-session-store-concurrency.md against the single-owner actor rollout and decide whether to prototype .agent-doc/state.db as the authoritative session store.
❯ Please review the % of usage from agent-doc. What are the inefficiencies of token usage? #next-steps
```
  What's contributing to your limits usage?
  Approximate, based on local sessions on this machine — does not include other devices or claude.ai

  Last 24h · these are independent characteristics of your usage, not a breakdown

  22% of your usage was while 4+ sessions ran in parallel
   All sessions share one limit. If you don't need them all at once, queueing uses
    it more evenly.

  13% of your usage came from subagent-heavy sessions
   Each subagent runs its own requests. Be deliberate about spawning them — and
   consider configuring a cheaper model for simpler subagents.

  67% of your usage came from /agent-doc
   Heavy skills can be scoped down or run with a cheaper model via skill
   frontmatter.

  Skills                  % of usage
  /agent-doc                     67%
  /chromium-bridge                1%
```
### Re: agent-doc usage — gpt-5

The 67% `/agent-doc` number is the main signal, but it should not be read as a clean pie-chart slice with the 22% parallel-session and 13% subagent-heavy numbers. The usage report says those are independent characteristics, so some `/agent-doc` usage may also be counted in the parallel or subagent-heavy buckets.

The biggest token inefficiency is that `agent-doc` pays a large fixed cost on nearly every cycle. Locally, the repeated hot-path instruction surfaces are all around the same size: root `AGENTS.md` is about 2,153 words, root `CLAUDE.md` is about 2,153 words, `.codex/AGENTS.md` is about 2,144 words, and `src/agent-doc/SKILL.md` is about 2,121 words. `src/agent-doc/CLAUDE.md` is larger at about 3,595 words. The core safety contract is important, but the same closeout, preflight, finalize, commit, and repair rules are repeated across several places.

Specific inefficiencies I see:

- duplicated harness instructions: Claude, Codex, root, and source skill copies repeat most of the same workflow with small wording changes
- rare recovery rules in the hot path: startup-miss, dispatch-only proof, passive sync layout protection, late owner proof, MCP auth closeout, and manual commit boundaries are safety-critical, but they do not all need full prose in every normal turn
- runbook fanout: the key referenced runbooks I checked total about 7,956 words, so casually opening several of them can exceed the size of the base skill again
- noisy preflight context: preflight may sweep and report unrelated session documents before returning the active document diff; useful for safety, but expensive if the full tool output stays in model context
- prompt-target duplication: this turn's plan exposed both the full user prompt and the quoted usage-report heading as prompt targets, which can cause duplicate interpretation work
- document accretion: even after compaction, the active document still carries frontmatter, summaries, backlog, and hidden historical comments; that is much better than an uncompacted exchange, but still a recurring base cost
- concurrency amplification: when 4+ sessions run in parallel, every fixed `agent-doc` cost is multiplied against the shared limit; subagent-heavy turns multiply it again because each subagent makes its own requests

Highest-leverage next step: execute `tasks/agent-doc/plan-instruction-surface-context-trim.md`. That plan directly targets the dominant waste: shrinking the always-loaded instruction layer while keeping binary-owned preflight, plan, finalize, and session-check invariants intact. I added `#instrcap` back to the top of the backlog for that reason.

Priority order after this cycle should be:

1. `#instrcap` - reduce recurring instruction/context overhead
2. `#chkptcap` - reduce work loss when long responses or panes die mid-generation
3. `#sqliterepros` - gather evidence before changing session-state architecture
4. `#sqliteeval` - decide whether the SQLite state-store prototype is still worthwhile

Top backlog item: `#instrcap`.
<!-- agent:boundary:67245359 -->
<!-- /agent:exchange -->
###

<!--
An agent-doc turn had an error for dev.md:
```
● Bash(agent-doc session-check
      /home/brian/work/btakita/agent-loop/src/session-share/tasks/buildparty-investor-demo/dev.md 2>&1)
  ⎿  Error: Exit code 1
     [session-check] INTERRUPTED: cycle `cycle-1778181134648` is `committed` (commit_success), but the document still
     has unresolved prompt-bearing user changes with no new agent-doc cycle started: prompt_target: ❯ **Verification:**
      Both redirects confirmed via `curl`. All 519 assertions pass (6 suites). Specs + news updated. Committed + pushed
      buildparty-investor-demo and session-share.
```
---
"❯ Commit / push:" should not have the `❯ ` prefix since this was the agent response. #agent-doc-bug
---
Please optimize the AGENTS.md + SKILL.md to reduce context usage yet maintain effectiveness for Claude Code + Codex
---
Does the supervisor actor ever block on `Run Agent Doc`? I noticed that /home/brian/work/btakita/agent-loop/tasks/software/tsift did not respond to `Run Agent Doc` in a previous turn. It seems to run ok now but I'm concerned that `Run Agent Doc` was non-responsive.
---
"Press Enter to restart, or 'q' to exit."
-->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#instrcap] Execute plan tasks/agent-doc/plan-instruction-surface-context-trim.md to trim always-loaded AGENTS/SKILL surfaces, move rare recovery prose behind targeted runbooks, and add a size/duplication budget check.
- [ ] [#chkptcap] Periodic checkpoint capture during response streaming — capture partial responses every 30s so mid-generation pane death does not lose work entirely.
- [ ] [#sqliterepros] [recommended] Before prototyping .agent-doc/state.db, capture 2-3 current agent-doc failures where cross-file authority drift or non-atomic state writes would have been fixed by one transactional store rather than by the single-owner actor/supervisor model alone.
- [ ] [#sqliteeval] [recommended] Review plan tasks/agent-doc/plan-sqlite-session-store-concurrency.md against the single-owner actor rollout and decide whether to prototype .agent-doc/state.db as the authoritative session store.

<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->



<!--
I see the following in the binding codex session prompt. Why is it there? Why was it not Entered? spec-test-build-install-commit-push
```
› echo '[agent-doc] startup-miss: routed trigger accepted but no document cycle started for pending prompt_target: do
  #ppct. spec-test-build-install-commit-push. Run '\''agent-doc start tasks/agent-doc/agent-doc-bugs2.md'\'' to retry.'
```
-->
