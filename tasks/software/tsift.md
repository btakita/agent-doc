---
agent_doc_session: tsift-v0.1
resume: 17b41e7b-2df9-42ec-8a28-879b1f729d43
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260801-050221.md`*

Compacted content:
- Archived 1 response topic(s): do [#finishsymbolkind]
- Prior summary/context: prior compacted content: Archived 5 response topic(s): do [#tsrenameast1]; do [#tsrenameast2]; do [#tsrenameast3] / d...

### Re: do [#zigthirdindexed] — opus-5

> **Queue prompt:**
>
> do [#zigthirdindexed]


Done, and it turned into a correction of the two grammars that shipped first. The
code landed last cycle; this cycle is the closeout that the retained-write wedge
blocked, so the commits below are already pushed.

**Zig, by the opposite rule.** Copying the callee-only narrowing to Zig would have
been a silent under-rename, not a fix. Zig has no import-into-namespace form —
`@import("m.zig").name` and `Type.name` are the only ways to name another file's
declaration, and both are `field_expression` members — so the callee rule would
rename a `const` or an uncalled function and leave every cross-file reference
behind. Zig is narrowed by the *receiver* instead: a member survives when the
receiver chain roots in an `@import` binding or a container type (in Zig a
container type is also its namespace), and is dropped when the receiver is an
ordinary value, where the member is a struct field. `container_field` names are
dropped for every resolved target, since the Zig symbol query never produces one.

**The same question was already open, and already wrong, in Python and Kotlin.**
Writing the receiver rule made both defects concrete rather than hypothetical:

- Python dropped `mod.name`. That *is* the module-level binding when `mod` is an
  imported module, and `import mod` is half of how Python spells a cross-module
  reference — on a rename path that is cross-file. It renamed the definition and
  left every reader broken. Now kept when the receiver roots in an `import`-bound
  name, alias-aware, and `import pkg.mod` correctly binds `pkg`.
- Kotlin dropped `Panel.widgetCount` and `Registry.widgetCount`, which reach a
  companion or `object` member the index holds as a declaration. Now kept when the
  receiver names a `class`/`object`/`interface` declared in the file.

**Verification.** `make check` green, exit 0, no failures, no clippy warnings. Each
of the four new tests was mutation-checked: disabling its own guard reddens it and
nothing else, so none is a false green. The cross-language conformance fixtures for
Zig, Python, and Kotlin were widened to assert renamed *and* untouched positions
individually. Landed in tsift `13e4cd7`, gitlink `45b7989`, plan note `68c1d84`.

**What is left, stated plainly.** A receiver whose binding lives in *another* file
cannot be resolved from one parse tree, so Zig and Kotlin still fall back to the
callee rule there. For Kotlin that residue is qualified access to an imported type,
not the common path — an `import` binds a bare identifier the walk never touched.
Closing it needs the call graph, not the grammar, which is phase 2's machinery
applied to occurrences rather than to files.
<!-- agent:boundary:aa447644 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ~~do [#zigthirdindexed]~~
- do [#wsdq]
- do [#jzs7]
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
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
