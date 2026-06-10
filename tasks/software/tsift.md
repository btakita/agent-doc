---
agent_doc_session: tsift-v0.1
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260610-042111.md`*

Compacted content:
- Archived 3 response topic(s): pcache-prefix-drift; contextpack-queue-slim; token-actions-rewrite
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: agent-doc coupling — gpt-5

> **Queue prompt:**
>
> Is there too much coupling with the agent-doc library?


Short answer: not too much at the core/library boundary, but there is still too much session-schema knowledge in `tsift-cli`.

What I checked:
- `cargo tree -i tsift-agent-doc --workspace`: only root `tsift`, `tsift-cli`, and compatibility `tsift-session` depend on it.
- `packages/tsift-agent-doc/Cargo.toml`: depends on `anyhow`, `serde`, `serde_json`, and `tsift-quality`; it does not link to an external agent-doc runtime/library.
- `VERSIONS.md` records `#agent-doc-domain-boundary`: the session observability domain was intentionally extracted into `packages/tsift-agent-doc`.

Verdict:
- Healthy: core indexing/search/graph crates are not built around agent-doc. The agent-doc-specific work is mostly a dedicated domain crate plus CLI surfaces that intentionally serve agent-doc sessions.
- Risky: `packages/tsift-cli/src/lib.rs` still parses agent-doc markdown/session shape directly (`agent_doc_session:`, `agent:queue`, backlog lines, worker-result lines) for traversal and graph packets. `packages/tsift-cli/src/rewrite.rs` also has its own “looks like agent-doc session/log” detection. That duplicates schema knowledge already owned by `tsift-agent-doc`.
- Public API stickiness: root `src/lib.rs` re-exports `session_cost`, `session_digest`, and `session_review` from `tsift-agent-doc`, so the root crate exposes the domain by default. That is not a runtime problem, but it makes the public package less domain-neutral.

Recommendation:
Do not remove agent-doc from tsift; agent-doc is a real target consumer and the session-review/context-pack workflows justify first-class support. The cleanup I queued is narrower: move the remaining CLI-side agent-doc markdown/backlog/queue parsing into typed helpers in `tsift-agent-doc`, then have traversal, context-pack, and rewrite call those helpers. That keeps the product integration while reducing schema coupling.

Proof:
- Changed paths: session document only; added one recommended backlog item.
- Commands: `tsift status`, `agent-doc --version`, `agent-doc preflight/plan/read`, `rg ... agent-doc`, `cargo tree -i tsift-agent-doc --workspace`, targeted source reads.
- Verification: no tests run because this was an architecture assessment with no code changes.
- Confidence: medium-high.
<!-- agent:boundary:fa57b0e4 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- [#x5fw]
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#x5fw] [recommended] Move CLI-side agent-doc session markdown/backlog/queue parsing into typed helpers exported by `packages/tsift-agent-doc`, then update traversal/context-pack/rewrite to consume those helpers.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
