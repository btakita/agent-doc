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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260530-182503.md`*

Compacted content:
- Archived 5 response topic(s): tsiftcli-direct-crates; #tsift-dormant-crates; closeout recovery; 2 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ Are there #next-steps for tsift to manage monorepos, multi-crate architectures, and other multiplicities?
### Re: monorepo and multiplicity next steps — gpt-5

Yes. tsift already has a solid first layer for monorepos: `.gitmodules`-backed workspace scopes, per-scope index DBs, federated search, duplicate leaf-name disambiguation, privacy/isolation tiers, path-hinted scope inference, and fail-closed unknown/ambiguous scope handling. The next step is not just “more scopes”; it is making nested multiplicities explicit in the graph and scheduler.

I added the backlog in priority order. Top item: `#tsift-multiplicity-inventory`.

Recommended order:

1. `#tsift-multiplicity-inventory` — define the hierarchy and precedence among repo root, git submodule, Cargo workspace, package/crate, language workspace, generated/runtime scope, and agent-doc session scope.
2. `#tsift-crate-graph` — project Cargo workspace/package/crate structure into the graph substrate as first-class nodes and edges.
3. `#tsift-scope-selectors` — extend selectors and path inference beyond git submodules while keeping ambiguous cases fail-closed.
4. `#tsift-multiplicity-orchestration` — feed scope/package/crate ownership into `conflict-matrix` and `dependency-dag` so parallel work can split by real ownership boundaries.
5. `#tsift-multiplicity-fixtures` — lock the behavior with fixtures for duplicate crate names, nested workspaces, private scopes, stale indexes, graph parity, and perf gates.

Proof: changed only this session document/backlog; no source edits. Commands run: `tsift status`, `tsift status --fix`, `agent-doc preflight`, `agent-doc orchestrate --from-exchange --plan`, `agent-doc plan`, `agent-doc read`, targeted tsift/search/source reads, and `git status --short`. Verification is document-level finalize/session-check only because this cycle captured follow-up work rather than changing code.
<!-- agent:boundary:704a022a -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#tsift-multiplicity-fixtures] Add a fixture and acceptance suite for monorepo/multiplicity behavior: duplicate crate names, nested workspaces, multi-language package managers, private/isolated boundaries, missing/stale scoped indexes, graph-db/conflict-matrix/dependency-dag parity, and full-projection perf gates.
- [ ] [#tsift-multiplicity-orchestration] Feed scope/package/crate ownership into conflict-matrix and dependency-dag so parallel worker packets can reason about cross-crate shared files, generated outputs, tests, config ownership, and safe antichains without collapsing the whole monorepo into one target.
- [ ] [#tsift-scope-selectors] Extend CLI/config selectors beyond git submodules: preserve existing --scope behavior, add crate/package/workspace aliases where appropriate, infer nested path targets deterministically, and keep unknown/ambiguous selectors fail-closed with available-scope guidance.
- [ ] [#tsift-crate-graph] Project Cargo workspace/package/crate structure into the graph substrate: workspace/package/crate nodes, membership edges, feature/target metadata, build/test targets, and cross-crate import/use edges separate from ordinary call edges.
- [ ] [#tsift-multiplicity-inventory] Write the multiplicity model before adding more indexing behavior: define the hierarchy and precedence among repository root, git submodule scope, Cargo workspace, package/crate, language package manager workspace, generated/runtime scope, and agent-doc session scope.
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
- [ ] [#npm-auth-setup] Configure NPM_TOKEN secret + TSIFT_ENABLE_NPM_PUBLISH=true variable in GitHub repo settings
- [ ] [#npm-publish-verify] After auth configured, tag release and verify publish-opencode-plugin CI job succeeds and opencode-tsift appears on npmjs.org
- [ ] [#full-e2e-verify] In fresh project: opencode plugin opencode-tsift && opencode then /tsift-status. On success mark #e2e-install done.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
