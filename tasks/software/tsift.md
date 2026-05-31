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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260531-151545.md`*

Compacted content:
- Archived 6 response topic(s): queued claude-mem migration; #tsiftmemimportall; #tsiftmemsemanticgate; 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ Does tsift support memory in opencode, codex, and claude code?


### Re: tsift memory harness support — opencode

Yes. tsift memory works across all three harnesses, but the integration layer differs:

| Harness | Integration | Memory access |
|---------|------------|---------------|
| **Claude Code** | `PreToolUse` hooks (`tsift-rewrite.sh`) auto-rewrite high-token commands; `UserPromptSubmit` hook auto-reindexes | Hook contract captures `prompt_target`, `tool_result_artifact`, `response_summary`, `closeout_proof`, `session_check` events via agent-doc closeout pipeline |
| **OpenCode** | `opencode-tsift` npm plugin installs `/tsift-memory-status`, `/tsift-memory-search`, `/tsift-memory-guard` commands | Commands shell out to `tsift memory status`, `tsift graph-db related`, `tsift memory budget-guard` — same `tsift-memory` crate, same `.tsift/memory.db` |
| **Codex** | `tsift init --codex` installs workspace hooks (`packages/tsift-index/src/init.rs`); no `PreToolUse` hooks, so uses `tsift rewrite --run '<command>'` manually | Same memory DB and graph-db retrieval path; `tsift rewrite --run` provides the bounded-output policy that Claude gets automatically via hooks |

The underlying substrate is identical across all three: the `tsift-memory` crate (`packages/tsift-memory/`) with a SQLite-backed `.tsift/memory.db` storing events (prompt targets, tool call artifacts, response summaries, closeout proofs, session checks, imported observations) linked to the graph DB via `memory_graph_links`. The `MemoryHookSpec` contract (`agent_doc_hook_contract()`) defines the capture schema that agent-doc closeout writes regardless of which harness is active.

Key differences:
- Claude Code gets automatic hook-driven capture and rewriting
- OpenCode gets dedicated slash commands via the plugin
- Codex relies on `tsift init --codex` hooks and `tsift rewrite --run` for the same bounded path
<!-- agent:boundary:39a3f955 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to improve token efficiency along with graph + neighborhood performance & effectiveness.
---
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

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
- [ ] [#npm-auth-setup] Configure NPM_TOKEN secret + TSIFT_ENABLE_NPM_PUBLISH=true variable in GitHub repo settings
- [ ] [#npm-publish-verify] After auth configured, tag release and verify publish-opencode-plugin CI job succeeds and opencode-tsift appears on npmjs.org
- [ ] [#full-e2e-verify] In fresh project: opencode plugin opencode-tsift && opencode then /tsift-status. On success mark #e2e-install done.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
