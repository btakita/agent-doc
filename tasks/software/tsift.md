---
agent_doc_session: tsift-v0.1
resume: 019e85d7-f9ee-79f3-b597-4ed352495bbd
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260603-011149.md`*

Compacted content:
- Archived 11 response topic(s): next steps; #astspansearch; #astspansearch closeout recovery; 8 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are #next-steps to save tokens, improve performance, and improve graph lookups?
### Re: next steps for token, performance, and graph lookup work — gpt-5

Top backlog item: `#tokegate`.

I added five active backlog items in priority order. The sequence is: instrument token/perf first, reuse packets inside one agent-doc cycle, promote ranked neighborhoods only after quality gates, then tune SQLite frontier/hop behavior based on measured hotspots, with readiness checks preventing stale or low-signal graph expansion.

The highest leverage work is not swapping databases yet. `src/tsift/SPEC.md` already requires backend-eval and hop-cap promotion gates, and the code already has `next_token_actions`, `GraphEffectivenessReadiness`, `GraphStore::ranked_neighborhood`, and conflict-matrix preparation caching. The gap is turning those surfaces into a visible release gate that proves lower token output and better lookup quality across real sessions.

Commands/proof: ran `agent-doc preflight`, `agent-doc read`, `agent-doc orchestrate --from-exchange --plan`, `agent-doc plan`, `tsift status --fix`, `tsift index --workspace .`, targeted `tsift --envelope search`, and source/spec reads. No code changed, so no build suite was required.
<!-- agent:boundary:e6647987 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#tokegate] Add a cross-surface token/performance gate for context-pack, session-review --next-context, graph-db evidence, conflict-matrix, and dispatch-trace: benchmark representative agent-doc sessions and graph queries; record prompt tokens, envelope bytes, runtime, cache-hit rate, raw-read avoidance, and useful-hit density; fail CI on regressions.
- [ ] [#gpackreuse] Reuse graph/context packets across a single agent-doc cycle: share graph-db status/refresh, evidence, conflict-matrix preparation, context-pack diff, and impact packets by source/document/staged-diff watermark; prove repeated calls report skipped phases and stable packet ids.
- [ ] [#rankdefault] Promote ranked_neighborhood into default graph lookup/context-pack/evidence output only after quality gates pass; compare ranked vs unranked output on real backlog/code queries for useful-hit density, duplicate-name precision, handle coverage, latency, and emitted tokens.
- [ ] [#gfrontierhot] Run three-sample performance triage for SQLite neighborhood/path/evidence/related lookups on real, full-projection, high-degree, and deep-chain workloads; optimize recursive frontier expansion or hop caps only where metrics show a hotspot.
- [ ] [#greadiness] Make graph-readiness states actionable at the lookup boundary: if summaries, tagpath handles, graph freshness, or scoped indexes are missing, emit exact repair commands and suppress broad low-signal graph expansion.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#surrealbulk] Batch SurrealDB projection load: replace per-row upsert loops with bulk/transactional row load plus one-pass secondary-index rebuild; measure refresh/load separately.
- [/] [#full-e2e-verify] In fresh project: opencode plugin opencode-tsift && opencode then /tsift-status. On success mark #e2e-install done.
- [/] [#npm-publish-verify] After auth configured, tag release and verify publish-opencode-plugin CI job succeeds and opencode-tsift appears on npmjs.org
- [/] [#npm-auth-setup] Configure NPM_TOKEN secret + TSIFT_ENABLE_NPM_PUBLISH=true variable in GitHub repo settings
- [/] [#e2e-install] Verify end-to-end install: fresh opencode project + opencode plugin opencode-tsift + /tsift-status
<!-- /agent:review -->
###
<!--
Can we increase max-hop caps?
Could FalkorDB help with performance?
-->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
