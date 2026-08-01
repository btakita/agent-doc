---
agent_doc_session: tsift-v0.1
resume: 1f5b2d57-c114-4e4d-88c4-dd9057d75cdc
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260801-043053.md`*

Compacted content:
- Archived 5 response topic(s): do [#tsrenameast1]; do [#tsrenameast2]; do [#tsrenameast3] / do [#tsrenameast4]; 2 more
- Prior summary/context: prior compacted content: Archived 10 response topic(s): do [#astgrepstructural]; ast-grep as the core, and more langu...

### Re: do [#finishsymbolkind] — opus-5

Already landed by the prior cycle in tsift `36e669d`; that cycle committed the code
but never closed the item out. This turn verified it and closed it.

**What is in the fix.** `occurrence_matches_target` gained a Python and a Kotlin arm
in `packages/tsift-graph/src/rename.rs`. Python drops an `identifier` that is the
`attribute` field of an `attribute` node; Kotlin drops a non-first `identifier` under
`navigation_expression`. Both keep it when the enclosing attribute/navigation
expression is the `function` of a call, because both languages index methods as
callables — so `panel.widget_count()` is renamed and `panel.widget_count` is not.
The two conformance fixtures in `semantic_edit.rs` were widened to assert the method
declaration, the direct call, the method call, and the untouched member read
individually rather than by count.

**Verification.** `make check` in `src/tsift` is green, exit 0, no failures, and both
new tests ran by name (`python_callable_narrowing_keeps_method_calls_but_skips_attribute_reads`,
`kotlin_callable_narrowing_keeps_method_calls_but_skips_navigation_reads`). CI run
`30684057240` is still in progress, so this closeout rests on local evidence.

**One defect fixed here.** That commit de-indented three continuation blocks in
`VERSIONS.md` out of the bullet they belong to, so they rendered as top-level
paragraphs. Restored in `582d7c4`, gitlink bumped in `d1fb343`.

**Scope note, filed rather than fixed.** The item called Python and Kotlin "the two
indexed languages still at a flat identifier set" — Zig is a third. It is a real gap,
but not a mechanical copy of the same rule: Zig has no import-into-namespace, so
`@import("m.zig").foo` is the *only* cross-file reference form, and it is a
`field_expression` member. The callee-only rule would drop every cross-file reference
of a renamed const or struct — silent under-rename on a cross-file rename path. That
needs receiver resolution (is the object a module or a value?), which is a design
decision, so it is at the top of the backlog with the same caveat noted for the
Python/Kotlin `mod.CONST` case the shipped rule already drops.

Unrelated to this document: preflight reports the installed `agent-doc` binary is one
commit behind. `src/agent-doc` has a dirty tree from another session, so I did not
build over it.
<!-- agent:boundary:6fffd799 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- 🚧 do [#zigthirdindexed]
- do [#wsdq]
- do [#jzs7]
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] 🚧 [#zigthirdindexed] Zig is a third indexed language with a flat identifier node set, and it is deliberately left un-narrowed. Zig has no import-into-namespace form, so every cross-file reference to another module's symbol is the member of a field_expression. Applying the Python/Kotlin callee-only rule there would silently drop every cross-file reference of a renamed const, struct, or non-called function, which is exactly the under-rename the module doc warns against. Decide the receiver-resolution rule first: narrowing a member position needs to know whether the object is a module namespace or a value. The same open question applies to the already-shipped Python and Kotlin rules, which today drop a module-qualified read such as mod.CONST.
- [ ] [#wsdq] release + publish + install
- [ ] [#jzs7] Plan and implement extract method refactoring
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
