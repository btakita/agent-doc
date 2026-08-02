---
agent_doc_session: tsift-v0.1
resume: a3a22fb5-698e-455d-9e58-aea3af22dc22
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260802-044013.md`*

Compacted content:
- Archived 4 response topic(s): do [#zigthirdindexed]; do [#wsdq]; 0.1.79 released, published, and installed; 1 more
- Prior summary/context: prior compacted content: Archived 1 response topic(s): do [#finishsymbolkind]

### Re: do [#registerextractfunction] — opus-5

Phase 2 landed. Python, GDScript, JavaScript, JSX, TypeScript, and TSX now share one extraction analysis; what varies is a node-kind vocabulary and an emitter, both `Dialect` data rather than Python node kinds inlined into the derivation. tsift `e272087`, `make check` green (448 in the CLI suite), CI run `30733679220` still in progress at close.

**The registration itself** is what phase 2 asked for: the JS-like tier moved onto the script kinds table (which now carries `extract_function`, collapsing the separate Python table that only existed to hold it), GDScript got `SEMANTIC_EDIT_INDEXED_EXTRACT_KINDS` — renamable and extractable, still not structurally matchable, since it has no ast-grep grammar in this build — and there is a conformance fixture row per extractable executor with both-directions exhaustiveness guards, the same shape the rename table settled on. Each row asserts its emitted function and its call site individually. I mutation-checked four of them (GDScript emitting `def`, JS dropping the `let`, TS dropping the copied annotation, the indent unit hard-coded to four spaces); all four redden the table.

**TypeScript is in the family only because it can copy.** It takes the annotation off the parameter or declaration that already binds the name; where there isn't one it refuses. `unknown` and an implicitly `any` parameter both type-check something other than what the code does, which is the failure this intent exists to avoid — so most real TS ranges will decline, by name, and that is the intended shape.

**GDScript refuses more than one returned name.** It has no destructuring assignment, so two values cannot reach one call site; an array plus two index reads would be three statements where the caller wrote one.

**Three defects the generalization surfaced**, each of which emitted code that parsed:

- A bare assignment does not declare outside Python. A range that only *assigns* a name declared behind it left the new function writing a name that was not in scope — `tsc --strict` calls that TS2304, plain JS calls it a global. The new function now opens with those declarations, and refuses outright when the name is not a local of the enclosing function, because declaring it would shadow an outer binding and not declaring it would write to a scope the caller did not choose.
- `obj.attr = 1` counted both `obj` and `attr` as bindings, so a receiver stopped being a parameter and the extracted body wrote to a name it never received.
- `base = base * 2` recorded its write at the target's own offset, so the right-hand read looked like a read of a local the range had already assigned and `base` fell out of the signature.

The first of those was found by running the binary, not by the suite — the unit tests and the fixtures were all green on the broken output. Worth stating plainly, because it is the second time the emitter agreed with itself and disagreed with the language.

Two refusals beyond the plan's list: hoisting out of a method or an expression-position function (a `def` placed beside a method *is* another method, and the bare call left behind does not resolve to it — this was silently broken for Python in phase 1), and the multiple-return case above. One improvement: a level of indentation is measured from the enclosing function's own body instead of assumed, so a tab-indented `.gd` file stays tab-indented.

**Live proof**, since the fixtures had already proven willing to be wrong: extracted Python and JavaScript both still compute `36` for `outer(3, 4)`, and the extracted TypeScript passes `tsc --noEmit --strict` while the pre-fix shape fails it with two `TS2304`s.

Next in the queue is `#deciderustspells` — phase 3, still a decision rather than an implementation. Two follow-ups queued behind it: hoisting past a class so extraction works inside methods, and the `break`/`continue` over-refusal when the loop is entirely inside the range.
<!-- agent:boundary:cd881543 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- 🚧 do [#extractfrommethod]
- do [#extractloopbreak]
- do [#deciderustspells]
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#deciderustspells] Decide how Rust spells extract_function before writing any Rust code: a caller-supplied signature, or a closure that infers. Rust cannot use the phase 1 derivation directly because choosing T, &T, or &mut T needs types tsift does not have, and guessing emits code that parses and does not build. A closure is correct without types but changes borrow behaviour and cannot be pub; a caller-supplied signature is honest but moves the hard part to the caller. Write the rationale, then implement. Plan: tasks/software/plan-tsift-extract-function.md
- [ ] 🚧 [#extractfrommethod] Make extract_function usable inside methods instead of refusing them: hoist the new function past the enclosing class rather than beside the method, so a Python extraction lands at module scope with `self` threaded through as an ordinary parameter, and a JS extraction lands beside the class declaration. The current EnclosingFunctionNotHoistable refusal is correct but costs the most common real extraction site. Plan: tasks/software/plan-tsift-extract-function.md
- [ ] [#extractloopbreak] Narrow the control-flow escape refusal: break and continue only escape when their loop is outside the selected range. A range containing a whole for or while loop refuses today even though hoisting it is safe, which is the most common false refusal in the phase 1 list. return and yield stay unconditional. Plan: tasks/software/plan-tsift-extract-function.md
- [/] [#8wqk] Register crates.io GitHub Trusted Publishing for every post-baseline tsift workspace crate through the operator browser/passkey flow, then verify a tag release publishes without the local token
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
