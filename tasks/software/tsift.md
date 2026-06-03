---
agent_doc_session: tsift-v0.1
resume: 019e85d7-f9ee-79f3-b597-4ed352495bbd
agent: opencode
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260603-041101.md`*

Compacted content:
- Archived 11 response topic(s): next steps for token, performance, and graph lookup work; #tokegate cross-surface token/performance gate; #gpackreuse cycle-scoped packet reuse; 8 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: reduce Code Navigation token usage — glm-5.1

> **Queue prompt:**
>
> /home/brian/work/btakita/agent-loop/src/boost-client/AGENTS.md & /home/brian/work/btakita/agent-loop/src/boost-client/src/monsterrodholders-dev/AGENTS.md has too much in the Code Navigation block. Use the runbooks and optimized session harness to reduce token usage.


Trimmed the Code Navigation blocks in both files from ~24 lines to 7 lines each, replacing the full inline command reference with a link to the canonical `runbooks/code-navigation.md`.

**Changes:**
- `src/boost-client/AGENTS.md` — 77→59 lines (~350 tokens saved)
- `src/boost-client/src/monsterrodholders-dev/AGENTS.md` — 148→130 lines (~350 tokens saved)

Both files now carry the `tsift:code-navigation` markers and a single `tsift status` call plus a relative link to the canonical runbook, following the Library Context Policy principle of no duplication.
<!-- agent:boundary:90bc191a -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
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
