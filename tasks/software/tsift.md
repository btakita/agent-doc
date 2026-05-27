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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260527-224338.md`*

Compacted content:
- Archived 4 response topic(s): next-steps to complete the review items; completed items should be marked as done in review + done; repair; 1 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
### Re: local-e2e-verify — opus-4-6

`tsift init --opencode /tmp/tsift-test` passed. Created:
- `AGENTS.md` with Code Navigation section
- 6 opencode commands: tsift-status, tsift-session-review, tsift-context-pack, tsift-diff-digest, tsift-test-digest, tsift-log-digest
- `.gitignore` with `.tsift/`

#local-e2e-verify done. #gopencode-install and #gopencode gated on npm auth. #npm-auth-setup requires manual GitHub NPM_TOKEN + TSIFT_ENABLE_NPM_PUBLISH. Remaining items (#npm-publish-verify, #full-e2e-verify) blocked on npm auth.
❯ Can tsift integrate well with https://github.com/aovestdipaperino/tokensave or should the features of tokensave be absorbed into tsift?
### Re: tsift + tokensave integration — opus-4-6

**Recommendation: integrate as complementary tools, not absorption.**

Both tools overlap on core code RAG (tree-sitter indexing, symbol search, call graphs, impact analysis) but diverge architecturally and in their sweet spots:

| | tsift | tokensave |
|---|---|---|
| Protocol | CLI (shell commands) | MCP server (48 tools) |
| Graph DB | SQLite + Convex projection | libSQL + FTS5 |
| Unique: envelopes | Budget-aware previews with stable handles | — |
| Unique: digests | 8 digest surfaces (diff, test, log, session, metric, cost, review, context-pack) | — |
| Unique: agent-doc | Graph projection of backlog/workers/dispatch | — |
| Unique: code analysis | — | 48 tools: health, DSM, god class, dead code, type hierarchy |
| Unique: edit primitives | — | str_replace, multi_replace, insert_at, ast_grep |
| Unique: multi-branch | — | Per-branch DBs with cross-branch diff |
| Unique: cost tracking | session-cost per-turn | Savings ledger, TUI monitor, dollar estimates |
| Languages | 10 | 34 |

**Integration path** — tsift already has a provider-neutral `GraphStore` trait (`substrate.rs`) with an experimental backend evaluation gate. tokensave's libSQL graph could become a backend candidate. Concretely:

1. **Graph backend plug-in**: Add a tokensave adapter to tsift's `GraphStore` trait. tsift calls tokensave's MCP `tokensave_search`/`tokensave_callers`/`tokensave_context` as its graph resolution layer instead of (or in addition to) its own SQLite graph.
2. **Envelope wrapping of tokensave results**: tsift's envelope system adds budget-aware truncation, stable handles, and follow-up expand commands to tokensave's already-rich tool responses. Agents benefit from both depth (tokensave tools) and token efficiency (tsift envelopes).
3. **Digest surface augmentation**: tsift's diff-digest, test-digest, and session-review already transform noisy output into bounded evidence. Running `tokensave impact`/`tokensave affected` inside those digest pipelines would give symbol-level change context inside tsift's bounded output format.
4. **Language expansion via tokensave**: tsift's 10-language tree-sitter support could delegate to tokensave's 34-language extraction for languages tsift doesn't cover natively.

**What NOT to absorb** — tokensave's 48 MCP tools, edit primitives, multi-branch indexing, and cost/TUI infrastructure are deep and well-maintained. Reimplementing in tsift would be months of work for marginal gain when tsift can call tokensave as a backend.

**What tsift should keep owning** — envelopes, digests, session-review, agent-doc graph projection, conflict-matrix, Convex sync. These have no tokensave equivalent and are tsift's core value proposition.

**Concrete first step**: implement a `tokensave` backend adapter in `substrate.rs` behind the existing `backend-eval` feature flag, then run it through `perf_gate.rs` to benchmark against the SQLite backend.
<!-- agent:boundary:0d95ecee -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- ~[#local-e2e-verify]~
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
