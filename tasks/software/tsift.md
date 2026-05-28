---
agent_doc_session: tsift-v0.1
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
queue_active: true
---

## Status

<!-- agent:status patch=replace -->

OpenCode npm package path code-complete and pushed. #gopencode-install and #gopencode gated on npm registry auth to publish opencode-tsift. 5 of 7 backlog items completed: integration tests, permission docs, transcript verify, smoke session, README expansion.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260528-212357.md`*

Compacted content:
- Archived 3 response topic(s): #step-2-backends; #step-3-tokensave-reader; #step-5-algorithms
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
### Re: #step-4-graph — glm-5.1

Confirmed `tsift-graph` crate is fully extracted and working:

- `packages/tsift-graph/` — ~2473 LOC across 3 modules:
  - `lib.rs` (1289 LOC) — CallSite, CallEdge, RouteSite extraction, community detection, path finding
  - `lang.rs` (832 LOC) — Lang enum, Symbol struct, language detection, tree-sitter queries
  - `complexity.rs` (352 LOC) — ComplexityMetrics, LanguageExtractor trait, LanguageRegistry
- `src/graph.rs` is a thin re-export shim (`pub use tsift_graph::*`)
- 67 unit tests pass in tsift-graph; 827 total workspace tests pass
- Build succeeds. Crate properly wired in workspace Cargo.toml with language feature forwarding

No new code changes needed — crate was already code-complete from prior session. Marked #step-4-graph done.

Next in queue: **#step-6-resolution** — Add `tsift-resolution` crate.
### Re: #step-6-resolution — glm-5.1

Extracted `tsift-resolution` crate with multi-strategy resolution, scoring heuristics, and blocklist. All tests pass. Built, installed, committed (`e5441ac`), and pushed.

**What changed:**
- `packages/tsift-resolution/` — new crate (~470 LOC) with:
  - `scoring.rs` (290 LOC) — RankedNeighbor, neighborhood ranking, edge kind scoring, heuristic helpers (community signal, semantic signal, freshness, handle coverage, duplicate name precision)
  - `blocklist.rs` (110 LOC) — generated artifact detection (.tsift, .agent-doc, target), planner config path filtering, index snapshot part checking
  - `resolve.rs` (110 LOC) — F1 scoring, token-overlap ranking, NodeMatchKind enum, kind priority, StrategyRank
- `src/resolution.rs` — thin re-export shim (`pub use tsift_resolution::*`)
- `src/main.rs` — inline struct definitions replaced with type aliases, inline functions replaced with delegation wrappers
- Workspace Cargo.toml updated with tsift-resolution member + dependency
- SPEC.md updated with new crate in architecture tree
- 56 unit tests in tsift-resolution (all passing); 883 total workspace tests pass

Next in queue: **#step-7-cli** — Extract `tsift-cli` crate.
<!-- agent:boundary:8ca26dcd -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~[#step-1-core]~
- ~[#step-2-backends]~
- ~[#step-3-tokensave-reader]~
- ~[#step-4-graph]~
- ~[#step-5-algorithms]~
- ~[#step-6-resolution]~
- ~[#step-7-cli]~
- [#npm-auth-setup]
- [#npm-publish-verify]
- [#full-e2e-verify]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#npm-auth-setup] Configure NPM_TOKEN secret + TSIFT_ENABLE_NPM_PUBLISH=true variable in GitHub repo settings
- [ ] [#npm-publish-verify] After auth configured, tag release and verify publish-opencode-plugin CI job succeeds and opencode-tsift appears on npmjs.org
- [ ] [#full-e2e-verify] In fresh project: opencode plugin opencode-tsift && opencode then /tsift-status. On success mark #e2e-install done.
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
