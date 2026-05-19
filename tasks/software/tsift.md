---
agent_doc_session: tsift-v0.1
resume: 019dd526-7958-77e3-9a41-92b8944fc550
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->
Project created. Repo: https://github.com/btakita/tsift. Submodule: src/tsift. Spec extracted to src/tsift/SPEC.md.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260519-223628.md`*

Compacted content:
- Archived 1 response topic(s): public tsift release
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ Please evaluate how tsift is used in agent-doc-bugs2.md? Notice the reoccurring attempts to fix the same issues. Did tsift disrupt Codex's and Claude's ability to fix the bugs?
### Re: tsift and recurring agent-doc fixes — gpt-5

Short answer: I would not blame `tsift` as the thing that disrupted Codex or Claude's ability to fix the recurring `agent-doc` bugs. The stronger read is that `tsift` is currently a diagnostic/context-shaping idea around `agent-doc-bugs2.md`, not a mature execution dependency in the bug-fix path. It exposed churn, and in some places it was too stale or too incomplete to prevent churn, but the root problem is still the `agent-doc` route/closeout/session architecture.

What is actually in `agent-doc-bugs2.md` now: `tsift` appears in the lower-agent orchestration backlog, especially `#v44k`, `#ffsm`, and `#ek86`. Those items say agent-doc should eventually use tsift JSON contracts, context packets, stable handles, freshness checks, and missing/stale fallback behavior. The linked plan is explicit that tsift is a future optional context-shaping layer: lower agents should receive bounded job packets, not whole sessions, and tsift should provide context packs, source handles, diff/test/log digests, session review, index status, and routing history. That is not the same as tsift already controlling the present `agent-doc-bugs2.md` fixes.

The recurring-attempt pattern is real. The done archive shows the same classes repeatedly returning under new IDs: startup/route readiness (`#startreadytimeout`, `#clearstartingrace`, `#opsreadinessrace`, `#startroute`, then `#rtbr`), late fallback or zero-patch closeout churn (`#latefallbackloop`, `#ds58`, `#djwb`, `#zpfb`), and prompt/scaffold ownership problems (`#spfxnorm`, `#bppfxstrip2`, `#lastpfx`, `#scaf2`, `#nd87`). The FlowCore plan says the same thing directly: route, write, git, sync, preflight, and session-check each own pieces of one lifecycle, so agents keep landing tactical fixes instead of changing the owning state machine.

I ran the installed `tsift 0.1.42` against the bug doc. `tsift session-review --next-context --json tasks/agent-doc/agent-doc-bugs2.md` correctly reported the live active target as `do [#mut2]. #spec-test-build-install-commit-push`; it did not replay old done IDs as current work. The fuller session review matched 12 sessions and surfaced the actual workflow smell: about 76M prompt tokens across matched sessions, 97% cached input ratio, largest turn around 244k tokens, fresh_restart=200, auto_trigger_timeout=23, ctrl_d_restart_loop=15, and commit_already_current=5675. That is useful diagnostic evidence of context/restart/closeout churn.

There is one important caveat: `tsift status --json` says the workspace index is stale, with 2629 stale files and missing generated instructions. So I would not trust tsift search/explain/graph output here without reindexing. If an agent had used stale tsift search as authoritative source localization, that could have wasted time. But in this document, the visible recurrence is not explained by tsift injecting bad code direction. It is explained by agents repeatedly fixing symptoms in a large, distributed lifecycle without a single typed owner for route proof, document mutation, closeout state, and prompt ownership.

My conclusion: tsift did not materially disrupt the bug fixing. It also did not yet solve the disruption it was meant to solve. The right sequencing is to keep `#mut2`/FlowCore first: make route and closeout state explicit in agent-doc itself. Then bring tsift in as a bounded evidence generator only after fresh-index gating, stable JSON contracts, and explicit missing/stale fallback behavior are implemented. Otherwise tsift remains a helpful mirror of churn, not a guardrail against it.
❯ Add #next-steps to make tsift more effective in bug fixes and code navigation.

❯ Read transcript in https://youtu.be/HQEm4rBKdec?si=rDH3qwVl6ndKliem
❯ Note how Graphify can navigate nodes of a code graph. Add #next-steps in making tsift more effective in code and knowledge graph traversal.
### Re: Graphify-style tsift next steps — gpt-5

I read the YouTube transcript for "Graphify Solves Claude's Biggest Limitation (Finally)." The relevant pattern for tsift is not just "make a graph"; it is making the graph directly navigable by agents: build graph artifacts from code and docs, keep raw JSON plus an inspectable HTML view, support shortest-path queries between two nodes, explain concepts from graph neighborhoods, and update only changed files instead of rebuilding or rereading everything.

I added two backlog items at the top. The top item is `#kgnv`: add Graphify-style tsift traversal primitives with stable graph node handles, graph HTML/JSON outputs, shortest-path and neighborhood explain APIs, and next-node recommendations for bug-fix navigation. The second is `#fidx`: gate agent-doc use of tsift on fresh incremental indexes so stale graph/search evidence cannot silently steer a bug fix.

That keeps `#98qd` as the broader agent-doc context-shaping work, but puts the concrete traversal/freshness pieces ahead of it so tsift can become useful as a code and knowledge graph navigation layer rather than only a diagnostic mirror.
<!-- agent:boundary:2b311982 -->
<!-- /agent:exchange -->
###
<!--
-->

## Backlog

<!-- agent:backlog -->
- [ ] [#kgnv] Add Graphify-style tsift traversal primitives: stable graph node handles for files/symbols/backlog/session artifacts, graph HTML/JSON outputs, shortest-path and neighborhood explain APIs, and next-node recommendations for bug-fix navigation.
- [ ] [#fidx] Gate tsift agent-doc use on fresh incremental indexes: update only changed files before search/graph packets, emit stale/missing diagnostics, and fall back to raw source when graph evidence is stale.
- [ ] [#98qd] Support agent-doc context shaping/orchestration: add tsift-backed backlog coverage for job packets, source handles, and bounded worker context.
- [ ] [#atvp] Unblock crates.io publication for tsift by replacing the upstream git-only sift dependency, publishing a compatible upstream crate, or vendoring a small stable search adapter; rerun cargo publish --dry-run before enabling TSIFT_ENABLE_CRATES_PUBLISH.
- [ ] [#g6kf] Confirm the v0.1.42 GitHub release completes after the macOS x86_64 Actions runner leaves the queue; if it remains stuck, remove or replace that matrix target and retag a fixed release.
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->
<!-- /agent:done -->
