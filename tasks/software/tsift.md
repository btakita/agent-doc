---
agent_doc_session: tsift-v0.1
resume: b8a61422-b7ed-4cdd-80a6-c793c56e04df
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
queue: go
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260730-215814.md`*

Compacted content:
- Archived 10 response topic(s): do [#astgrepstructural]; ast-grep as the core, and more languages; do [#astgreplangs]; 7 more
- Prior summary/context: prior compacted content: Archived 1 response topic(s): integrate ast-grep for search + refactor + editing
<!-- agent:boundary:8e8fbe9d -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ⏭️ do [#tsrenameast1]
- 🚧 do [#tsrenameast2]
- do [#tsrenameast3]
- do [#tsrenameast4]
- ~~Add support for gdscript.~~
- ~~Plan to for rename_symbol.~~
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#tsrenameast1] #tsrenameast1 rename_symbol phase 1 — AST occurrence collection: replace the substring scans in replace_rust_identifier/replace_script_identifier with tree-sitter identifier-node collection so string literals and comments stop being renamed, and report the true multi-line range. Plan: tasks/software/plan-tsift-rename-symbol-ast.md
- [ ] 🚧 [#tsrenameast2] #tsrenameast2 rename_symbol phase 2 — cross-file rename via the call graph: resolve referencing files from call edges and emit one all-or-nothing multi-file patch; refuse (naming the site) where the graph cannot prove the reference is the same symbol. Plan: tasks/software/plan-tsift-rename-symbol-ast.md
- [ ] [#tsrenameast3] #tsrenameast3 rename_symbol phase 3 — open rename_symbol to every indexed language (bash, zig, gdscript) via a per-language identifier-node-set entry plus conformance fixture rows asserting renamed AND untouched positions. Depends on #tsrenameast1. Plan: tasks/software/plan-tsift-rename-symbol-ast.md
- [ ] [#tsrenameast4] #tsrenameast4 rename_symbol phase 4 — add the end-to-end test for the structural-only rename_symbol refusal, reachable for the first time once an indexed language could otherwise fall through. Depends on #tsrenameast3. Plan: tasks/software/plan-tsift-rename-symbol-ast.md
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

<!-- agent:notes -->
• Ran tsift rewrite --run 'git log -5 --oneline --decorate'
└ tsift rewrite: no rewrite: no supported tsift rewrite matched this command; `--run` executes only rewritten commands; run the original command directly if intended

#spec-test-build-install-commit-push
---
route duplicate prompt residue guard failed: failed to parse components: unclosed component: `agent:notes` without matching close
<!-- /agent:notes -->
