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

### Re: do [#tsrenameast1] — opus-5

Phase 1 landed — `rename_symbol` now renames through the grammar instead of through the characters. tsift `6f2a4f2`, gitlink `0f4466c33`, pushed.

`packages/tsift-graph/src/rename.rs` collects occurrences from the parse tree, restricted to each grammar's identifier node kinds. Comments and string bodies are *different node kinds*, so they drop out by construction — there is no comment or string special case anywhere in the walk, and a new quoting or comment form cannot reintroduce the bug. Both family scans in `semantic_edit.rs` now route through it; `match_indices` is gone from the rename path.

**One risk I checked before committing to the design.** tree-sitter parses Rust macro arguments as an opaque `token_tree` — that's the documented reason structural patterns under-report inside `assert_eq!`. If identifiers in there weren't real nodes, an AST walk would have *lost* call sites the old substring scan found: a regression, not a fix. I probed the parse tree first; identifiers inside a `token_tree` are still named `identifier` nodes (byte offsets 176 and 208 in the probe fixture). No regression, and that case now has its own test.

Live, on the exact repro from last cycle:

    target range: 2-2
    edited range: 2-7

    /// Doc comment mentioning widget_count on purpose.   <- preserved
    pub fn gadget_count() -> usize { 3 }                  <- renamed
        // widget_count in a line comment                 <- preserved
        let label = "widget_count";                       <- preserved
        format!("{label}: {}", gadget_count())            <- renamed

**I need to correct something I wrote last cycle.** I called `range: 2-2` a defect — "a number that is not the edit". That was wrong: the field is `target_range`, the resolved symbol's *declaration* span, and it was reporting exactly that. The real gap was narrower — nothing reported what the edit touched, and the printer labelled the declaration span `range:`, which reads as the extent of the change. So: relabelled to `target range:`, and added `edited_range` computed from the two revisions of the file. I've amended the plan file rather than leave the overstated claim standing.

**Known limitation, carried into phase 2.** The Rust identifier set includes `field_identifier`, so renaming a function also renames an identically-named struct field. That matches the old scan's behaviour exactly, so it's not a new problem — but narrowing it needs the graph-scoped resolution that `#tsrenameast2` introduces, which is the right place for it.

Verification: `make check` green (clippy + full suite, exit 0), 7 new regression tests asserting each renamed *and* each untouched position individually per language — Rust, Python, TypeScript. I mutation-checked them: adding `string_content`/`line_comment` to the Rust identifier set reddens the Rust test (exit 101), so they drive real behaviour rather than counting replacements. A test that only counted replacements would have passed while renaming the wrong three positions.

Next head is `#tsrenameast2` — cross-file rename via the call graph, the defect where a rename silently leaves every caller broken.
<!-- agent:boundary:2c4fd517 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ~~⏭️ do [#tsrenameast1]~~
- 🚧 do [#tsrenameast2]
- do [#tsrenameast3]
- do [#tsrenameast4]
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] 🚧 [#tsrenameast2] #tsrenameast2 rename_symbol phase 2 — cross-file rename via the call graph: resolve referencing files from call edges and emit one all-or-nothing multi-file patch; refuse (naming the site) where the graph cannot prove the reference is the same symbol. Plan: tasks/software/plan-tsift-rename-symbol-ast.md
- [ ] [#tsrenameast3] #tsrenameast3 rename_symbol phase 3 — open rename_symbol to every indexed language (bash, zig, gdscript) via a per-language identifier-node-set entry plus conformance fixture rows asserting renamed AND untouched positions. Depends on #tsrenameast1. Plan: tasks/software/plan-tsift-rename-symbol-ast.md
- [ ] [#tsrenameast4] #tsrenameast4 rename_symbol phase 4 — add the end-to-end test for the structural-only rename_symbol refusal, reachable for the first time once an indexed language could otherwise fall through. Depends on #tsrenameast3. Plan: tasks/software/plan-tsift-rename-symbol-ast.md
- [ ] [#wsdq] release + publish + install
- [ ] [#jzs7] Plan and implement extract method refactoring
- [ ] [#w505] release + publish + install
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
