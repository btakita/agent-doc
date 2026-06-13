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
> #advance-review


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
3. Force-move the `v0.1.70` tag to HEAD and push to re-trigger; confirm the publish-crates.io job authenticates via OIDC and publishes.
2. `gh variable set TSIFT_ENABLE_CRATES_PUBLISH --body true`.
1. Register a Trusted Publisher on crates.io for each of the 23 tsift crates (repo `btakita/tsift`, workflow `release.yml`, no environment): `tsift-core`, `tsift-md-ast`, `tsift-graph`, `tsift-sqlite`, `tsift-algorithms`, `tsift-resolution`, `tsift-cache`, `tsift-tokensave`, `tsift-libsql`, `tsift-index`, `tsift-summarize`, `tsift-quality`, `tsift-agent-doc`, `tsift-digest`, `tsift-search`, `tsift-status`, `tsift-session`, `tsift-memory`, `tsift-memgraphrag`, `tsift-surrealdb`, `tsift-cli`, `tsift`, `tsift-sim-world`.
<!-- agent:boundary:4fabceda -->
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
- [ ] [#vmax] [operator-verify] Enable crates.io OIDC publishing for tsift (advances review #1td4; code already shipped in fe4144b/f29f7a2, release.yml crates job uses rust-lang/crates-io-auth-action@v1). Operator-gated steps (need crates.io account + browser 2FA — agent cannot perform): (1) On crates.io register a Trusted Publisher for EACH of the 23 tsift crates — GitHub repo btakita/tsift, workflow release.yml, no environment: tsift-core, tsift-md-ast, tsift-graph, tsift-sqlite, tsift-algorithms, tsift-resolution, tsift-cache, tsift-tokensave, tsift-libsql, tsift-index, tsift-summarize, tsift-quality, tsift-agent-doc, tsift-digest, tsift-search, tsift-status, tsift-session, tsift-memory, tsift-memgraphrag, tsift-surrealdb, tsift-cli, tsift, tsift-sim-world. (2) Enable the gate var: gh variable set TSIFT_ENABLE_CRATES_PUBLISH --body true. (3) Force-move the v0.1.70 tag to HEAD and push to re-trigger; confirm the publish-crates.io job authenticates via OIDC (no CARGO_REGISTRY_TOKEN) and publishes.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#1td4] Release infra: crates.io publish job in .github/workflows/release.yml is MIGRATED to OIDC trusted publishing (commit fe4144b, on origin/main f29f7a2; uses rust-lang/crates-io-auth-action@v1, id-token: write, no CARGO_REGISTRY_TOKEN). Code complete. Remaining is operator-gated and tracked as actionable backlog #vmax: register the Trusted Publisher per crate on crates.io (23 crates), set repo var TSIFT_ENABLE_CRATES_PUBLISH=true, then re-tag v0.1.70 to verify OIDC publish. Stays gated until operator completes crates.io registration.
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
