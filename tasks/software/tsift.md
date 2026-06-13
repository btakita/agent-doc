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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260613-173413.md`*

Compacted content:
- Archived 1 response topic(s): release + publish
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: #bugs-observed + improve prompt caching — opus-4-8

Added 4 prioritized backlog items.

**Observed bug (top priority, #1td4):** the v0.1.70 `Release` run (`27473681731`) **failed at crates.io with `403 authentication failed`** — `CARGO_REGISTRY_TOKEN` is expired/invalid. The other jobs succeeded: GitHub release `v0.1.70` is live, the OpenCode npm plugin published via OIDC, all platform builds passed. So the tag + commit are pushed and the GitHub/npm artifacts shipped, **but no crate was published to crates.io**. Root-fix: migrate the crates.io publish job in `release.yml` to OIDC trusted publishing (mirror the npm OIDC job) so there is no long-lived token to expire. Re-publishing is operator-gated — the trusted publisher must be registered on crates.io (or the token rotated) before a re-run; I did not retry the failed publish.

**Improve prompt caching (#0g7c, #avbq, #wwm1):** tsift's prompt-cache analysis (`packages/tsift-agent-doc/src/session_cost.rs`) currently *diagnoses* drift/regression but stops short of action. Queued:
- `#0g7c` — emit actionable remediation from `derive_prompt_cache_prefix_drift` (which field to stabilize / move below the breakpoint), not just attribution.
- `#avbq` — persist per-session effectiveness (ratio / net cached tokens / read-create regressions) so cross-run regressions are detectable; today the gate is per-fixture only.
- `#wwm1` [recommended] — surface caching health + top drift attribution inline in `session-review --next-context` / `context-pack` so it shows without running the fixture command.
- Tests: extended the two attribution tests to assert field-specific remediation, plus a new `prompt_cache_prefix_drift_remediation_is_field_specific` unit test (incl. fallback).
- Spec (`specs/digests-sessions.md`) documents the always-populated remediation.
- Rendered in both digest sites: compact (`prompt-cache-prefix-drift ... fix:<…>`) and verbose (`fix: <…>` line under each drift's field changes).
  - fallback → always actionable, never empty (covers future fields).
  - `stable_prefix_fingerprint` → audit the cached material; keep the fingerprint stable.
  - `stable_prefix` → move the volatile prefix bytes below the cache breakpoint.
  - `provider` → don't switch providers mid-session.
  - `routing_affinity` → pin the session to one cache replica.
  - `breakpoints` → keep breakpoint placement constant; move the volatile block below the cache breakpoint.
  - `cache_key` → reuse the same thread/session id so the warm prefix is served.
- New `remediation: String` on `SessionCostPromptCachePrefixDrift`, populated from the attributed `first_changed_field` via `prompt_cache_prefix_drift_remediation`:

No tsift code changed this turn (backlog capture only). Top of backlog: **#1td4**.
<!-- agent:boundary:27128d78 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- ~~:round_pushpin: do [#1td4]~~
- :round_pushpin: do [#0g7c]
- :round_pushpin: do [#avbq]
- :round_pushpin: do [#wwm1]
- #bugs-observed
- Add backlog items for tsift to improve prompt caching?
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#avbq] Track prompt-cache effectiveness across sessions: persist per-session cached_input_ratio / net_cached_input_tokens / read_create_regressions so cross-run regressions are detectable (today the gate is per-fixture / per-transcript only).
- [ ] [#wwm1] Surface prompt-cache effectiveness + top prefix-drift attribution inline in session-review --next-context / context-pack output (packages/tsift-agent-doc/src/session_cost.rs build_prompt_cache_effectiveness_report), so caching health shows without running the fixture command. [recommended]
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
