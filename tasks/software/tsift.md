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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260601-162307.md`*

Compacted content:
- Archived 1 response topic(s): lazily-rs cache invalidation
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ do [#lazily-cache-contracts]. #spec-test-build-install-commit-push
❯ Would SurrealDB be a better fit than Sqlite? See https://surrealdb.com/blog/surrealdb-3-x-by-the-numbers
### Re: lazily-rs cache contracts and SurrealDB — gpt-5

Done. I added a dedicated `### lazily-rs Cache Contracts` section to `src/tsift/SPEC.md` that records each lazily-rs-backed cache key, lifetime, invalidation trigger, and regression proof for `SummaryCache`, `InspectScopeGuard`, `StatusCheckCache`, `ResolveEdgesCache`, `GraphStore::neighborhood`, and `GraphStore::ranked_neighborhood`.

I also added `spec_documents_lazily_rs_cache_contracts` in `packages/tsift-cli/tests/exit_code.rs`. It verifies the packaged CLI spec stays synced with the workspace spec and asserts the cache-contract table keeps the required key/invalidation/test coverage strings.

On SurrealDB vs SQLite: not a default replacement for tsift yet. The May 29, 2026 SurrealDB 3.x post reports durable, same-hardware benchmark runs and claims strong embedded-mode write/scan results versus SQLite, which is interesting evidence for a future adapter spike: https://surrealdb.com/blog/surrealdb-3-x-by-the-numbers. But tsift's current promotion bar is broader than CRUD throughput: provider-neutral `GraphStore` parity, projection load/write behavior, install portability, and multi-process/read-only lock behavior across real, full-projection, high-degree, and deep-chain workloads. SQLite remains the safer default until a SurrealDB adapter clears that existing backend-eval gate.

Changed paths:
- `src/tsift/SPEC.md`
- `src/tsift/packages/tsift-cli/tests/exit_code.rs`
- superproject gitlink `src/tsift`

Verification:
- Passed: `tsift rewrite --run 'cargo test -p tsift-cli spec_documents_lazily_rs_cache_contracts'`
- Passed: `tsift --envelope __digest-runner --kind test --path . --shell-command 'make check'`
- Passed: `tsift rewrite --run 'cargo build --workspace'`
- Passed: `tsift rewrite --run 'cargo install --path . --locked --force'`
- Passed: `tsift --version` reports `tsift 0.1.62`
- Format caveat: workspace `cargo fmt --check` and file-level `rustfmt --check packages/tsift-cli/tests/exit_code.rs` are red on pre-existing formatting drift outside this change; I did not apply a broad formatting rewrite.

CI / tmux evidence:
- Latest pushed tsift CI run: https://github.com/btakita/tsift/actions/runs/26769887559
- The initial run and one failed-job retry both failed from GitHub runner infrastructure before project commands completed, with `System.IO.IOException: No space left on device` while the Actions runner wrote diagnostic logs. I treated this as external CI-start/runner failure, not a code failure.
- I found no `tmux`/`tmux-ci` target or tmux-specific job in this repo's configured CI workflow.
- The superproject has no workflow named `CI`.

Commits / push:
- Pushed `btakita/tsift@88838e8` (`docs: document lazily cache contracts`)
- Pushed `btakita/agent-loop@bbb2b65a8` (`chore: update tsift submodule`)

Escalation: none.
Confidence: high for the local implementation and verification; GitHub CI is blocked by external runner disk exhaustion rather than a project test signal.
<!-- agent:boundary:31982811 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [x] [#lazily-cache-contracts] [recommended] Document each lazily-rs cache key, lifetime, invalidation trigger, and matching regression test in src/tsift/SPEC.md so future cache additions have an explicit audit checklist.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
