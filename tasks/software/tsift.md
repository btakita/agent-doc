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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260601-222220.md`*

Compacted content:
- Archived 5 response topic(s): #surreal-backend-eval; #surreal-parity-suite; #surreal-native-adapter; 2 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ Is surrealdb at parity with sqlite?

### Re: SurrealDB parity — gpt-5

Short answer: yes for the current backend-eval parity surface, no for production replacement parity with SQLite.

The conformance path asserts SurrealDB's backend-eval report has `parity.matches_sqlite == true` and no diagnostics for each dataset, then compares SurrealDB row counts against SQLite for the parity operations. The spec says the SurrealDB prototype is compared against SQLite across status/count rows, edge lookup, edge-property scan, incident scan, neighborhood, related retrieval, path tiers, evidence, conflict-matrix, and dispatch-trace.

But that is still backend-eval/prototype parity. It is not promoted to SQLite-equivalent substrate status. The promotion gate keeps SurrealDB behind backend-eval until a real optional adapter proves provider-neutral projection writes/load, full `GraphStore` parity, writer/read-only lock behavior, default build/install portability, full-projection wins, and faster-than-SQLite results across the real, full-projection, high-degree, and deep-chain workloads. The code also labels the default SurrealDB path as a read snapshot unless the `backend-surrealdb` feature is enabled.

So I would treat it as: `SurrealDB is parity-checked and currently passes the measured read/query parity suite; SQLite remains the stable baseline and default until SurrealDB passes the production adapter and performance gates.`

Evidence checked: `SPEC.md`, `packages/tsift-cli/src/lib.rs`, and `packages/tsift-cli/tests/graph_db_conformance.rs`. No repo files changed.
<!-- agent:boundary:03031379 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
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
