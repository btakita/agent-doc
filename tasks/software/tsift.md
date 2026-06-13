---
agent_doc_session: tsift-v0.1
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#bugs-observed': Please add any bugs from observed behavior to the backlog.,
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

## Status

<!-- agent:status patch=replace -->

complete
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260613-183544.md`*

Compacted content:
- Archived 2 response topic(s): #bugs-observed + improve prompt caching; do [#avbq]
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: #bugs-observed — opus-4-8

> **Queue prompt:**
>
> :pushpin: Add backlog items for tsift to improve prompt caching?


> **Queue prompt:**
>
> :pushpin: #bugs-observed



No new **tsift** bugs this session — tsift itself (search, session-review, context-pack, build/install) behaved correctly. The prompt-caching backlog it spawned earlier (#0g7c remediation, #avbq cross-run persistence, #wwm1 inline surfacing) is now fully shipped and pushed (tsift `f29f7a2`).

Note for the record: #wwm1's `### Re:` response section was lost from this document when a compaction ran mid-closeout (archive `…-183544.md`) between the `--done` strike and `finalize`; the **code** shipped fine (f29f7a2, backlog marked done), only the in-doc narrative was dropped. That is an **agent-doc** workflow bug, not a tsift bug:

- **Observed:** a bare `agent-doc write --done <id>` (no response body) applied the backlog strike, a compaction then rewrote the exchange, and the next `agent-doc finalize` refused with `snapshot is 8684 bytes but the visible file is 3516 bytes … looks like a manual cleanup`. Recovery was `reset --from-current --preserve-session` → re-preflight.
- **Scope:** per session scope discipline I am **not** adding this to the tsift backlog (wrong project) and am **not** cross-repo editing `agent-doc-bugs2.md` from this tsift session. It should be filed via `#agent-doc-bug` in the agent-doc session (compaction-vs-finalize snapshot-drift race when a `--done` strike precedes the response write).

So: nothing added to the tsift backlog for this item — no tsift-scoped observed bug to capture (`no-pending-capture`).
- **Scope:** per session scope discipline I am **not** adding this to the tsift backlog (wrong project) and am **not** cross-repo editing `agent-doc-bugs2.md` from this tsift session. It should be filed via `#agent-doc-bug` in the agent-doc session (compaction-vs-finalize snapshot-drift race when a `--done` strike precedes the response write).
- **Observed:** a bare `agent-doc write --done <id>` (no response body) applied the backlog strike, a compaction then rewrote the exchange, and the next `agent-doc finalize` refused with `snapshot is 8684 bytes but the visible file is 3516 bytes … looks like a manual cleanup`. Recovery was `reset --from-current --preserve-session` → re-preflight.
<!-- agent:boundary:2bb31c02 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- #advance-review
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#1td4] Release infra: migrate the crates.io publish job in .github/workflows/release.yml to OIDC trusted publishing (mirror the OpenCode npm OIDC job) to root-fix CARGO_REGISTRY_TOKEN expiry. v0.1.70 Release run 27473681731 failed at crates.io with 403 authentication failed (GitHub release + npm OIDC succeeded); tag/commit are pushed but no crate published. Operator must register the trusted publisher on crates.io; until then crates.io publish stays blocked.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#g5kw] What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
- [ ] [#4z0w] Can we increase max-hop caps?
- [ ] [#cg52] Could FalkorDB help with performance?
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
