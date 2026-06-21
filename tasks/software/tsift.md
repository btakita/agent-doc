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
- Phase 4 cutover shipped — FTS5 `index.db` BM25 is the **default** lexical search path; JSON `TokenIndex` demoted to a freshness-gated degraded-read-only fallback.
- Phase 4b (b) path-prune (`#ve5f`) shipped; Phase 4b (a) decided + recorded (keep `TokenIndex::build` for degraded read-only).
- All pushed to `origin/main`.
- **Phase 5 — crates.io release.** Review item `#1yks` gates it: the two new crates `tsift-local-model` + `tsift-kg` need crates.io Trusted Publishing config before the `v*` tag, or the publish fails on them.
### Session Summary

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260621-035054.md`*

Compacted content:
❯ - Archived 9 response topic(s): session compaction; queue re-mirror; do #015t; 6 more
❯ - Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54...

### Re: session compaction — state recap — opus-4-8

The exchange was compacted (9 response topics archived to `…-20260621-035054.md`) and the compacted document is now committed cleanly (the snapshot/CRDT drift from the compaction was reset from the current file first, so HEAD matches the visible doc). This response closes the prompt-only summary tail. No new work was requested.

**`#015t` (FTS5 content index) state — Phases 0–4b COMPLETE:**
- Phase 4 cutover shipped — FTS5 `index.db` BM25 is the **default** lexical search path; JSON `TokenIndex` demoted to a freshness-gated degraded-read-only fallback.
- Phase 4b (b) path-prune (`#ve5f`) shipped; Phase 4b (a) decided + recorded (keep `TokenIndex::build` for degraded read-only).
- All pushed to `origin/main`.

**Only remaining item is operator-gated → `#015t` stays `[operator-verify]`:**
- **Phase 5 — crates.io release.** Review item `#1yks` gates it: the two new crates `tsift-local-model` + `tsift-kg` need crates.io Trusted Publishing config before the `v*` tag, or the publish fails on them.

The queued `[#015t]` is the go-mode mirror of the operator-verify umbrella (`queue_drainable_head_count: 0`, `queue_continuation_required: false`) — not a fresh directive, so I'm not looping. Say **"release"** (after `#1yks` is configured) to ship Phase 5.
<!-- agent:boundary:ef6c5dc7 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- :pushpin: [#015t]
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#015t] [operator-verify] FTS5 content index (plan: tasks/software/plan-tsift-fts5-content-index.md). FTS5 index.db BM25 is the DEFAULT lexical path; degraded read-only routes to the live TokenIndex fallback. Phase 4b COMPLETE: (a) degraded-read-only engine DECIDED 2026-06-20 — keep TokenIndex::build live-rebuild over rg -F for OR-union/ranking parity (tsift 1caaa43); (b) files_in_path_scope path-prune DONE (#ve5f, tsift d908371). NEXT = Phase 5 = operator-gated crates.io release ONLY. Say "release" to ship, or schedule it.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#1yks] [operator] Finish tsift v0.1.72 publish: (1) bootstrap crates.io Trusted Publishing for the two NEW crates tsift-local-model + tsift-kg (browser/passkey 2FA — the prior 23 crates are configured, these two are not and a brand-new crate's first OIDC publish needs its config to exist; if crates.io rejects pre-config for a nonexistent name, do one manual 'cargo publish -p <crate>' to create it); (2) push the v0.1.72 tag (git tag v0.1.72 08d0f65 && git push origin v0.1.72) to trigger Release CI. Without step 1 the crates.io job fails again on the two new crates.
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
