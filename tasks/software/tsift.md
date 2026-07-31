---
agent_doc_session: tsift-v0.1
resume: 2a62090c-ec88-4312-8b64-de627376e280
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

### Re: do [#tsrenameast2] — opus-5

Phase 2 landed. tsift `784a6cf`, gitlink `0f23f4d54`, pushed. `make check` green (exit 0, clippy clean).

A rename now resolves its extent from the indexed call graph and rewrites every referencing file in one all-or-nothing buffer set. On the defect-3 repro, `caller.rs` gets both its `use crate::gadget_count;` and its call site renamed, and **the resulting tree compiles** (`cargo check` exit 0) — which it did not before.

**On your sequencing call:** `#wsdq` is reordered behind the rename phases and `#w505` is reaped as an exact duplicate of it. Release comes after `#tsrenameast4`.

**Two safety rules came out of a red suite, not out of design.** Both are worth recording because they falsify the tidy story in the plan.

`callers_of(name)` and `symbol_info(name)` are matched **by name, not by resolved binding**. My first cut scoped only by file, so:

1. A Python `beta` blocked renaming a JavaScript `beta` — and would have had the rename rewrite the unrelated Python file. Scoping is now per executor family, which keeps `.ts`/`.tsx`/`.js`/`.jsx` together since they genuinely call each other.
2. Family scoping alone still failed: a file that defines its **own** `beta` and calls it looks like a caller of ours. Its own definition shadows any import, so it is calling itself — renaming there rewrites an unrelated function. Those files are now excluded.

Both were caught by the pre-existing `edit_intents_apply_mutates_javascript_executor_intents` suite. I would not have found either from the fixtures I wrote myself; each now has a dedicated regression test naming the suite that caught it.

**Refusal is narrower than I planned.** The plan said refuse whenever a second definition of the name exists. That is wrong — two independent modules each with their own `beta` is ordinary, and with no cross-file reference to attribute, the rename never leaves the declaring file. So the refusal fires only when there is a cross-file caller **and** a competing same-family definition, and it names both:

    rename_symbol refuses "widget_count": it is referenced from src/caller.rs, and the
    index holds another definition of that name in src/other.rs; a call edge cannot say
    which definition those references belong to

Nothing is written on a refusal — asserted, not assumed.

12 rename tests now, each asserting renamed *and* untouched positions individually. I mutation-checked the new ones: disabling the cross-file branch reddens the cross-file test, disabling the ambiguity filter reddens the refusal test, and disabling the shadowing rule reddens the self-defining-caller test. All three fail on their mutant and pass on the real code.

Remaining: `#tsrenameast3` (open rename to bash/zig/gdscript), `#tsrenameast4` (the structural-only refusal E2E), then `#wsdq` release.

### Re: do [#tsrenameast3] / do [#tsrenameast4] — opus-5

Both phases landed together — phase 4 turned out to be a correction to phase 4, so splitting them would have shipped a claim I could not support. tsift `b7b02c7`, gitlink `0d92121df`, pushed. `make check` exit 0, 1644 tests, clippy clean.

`rename_symbol` now works for Kotlin, Bash, Zig, and GDScript. Nothing about those languages was blocking it — each family hand-rolled its own substring scan, so a language without one was refused while being fully indexed. Once occurrences come out of the grammar and extent comes out of the call graph, the language is a parameter: registration is an identifier-node-kind set plus a fixture row, and no new rewriting code exists anywhere in this change. Kotlin came along because it was indexed and structural for the same implementation reason, and leaving it out would have been an arbitrary line.

**Two things fell out of building it that the plan did not have, and both are the interesting part.**

Bash needed a *position* rule, not a kind rule. Its grammar spells the function name, the command name, and every unquoted argument as the same `word` node — so `echo deploy` would have had the rename rewrite an argument that is data. That is phase 1's string-literal defect arriving through a different door, and a kind-set table entry alone would have shipped it. `word` occurrences are now restricted to the declaration and command-name positions; `variable_name` needs no guard, since it is only ever a variable.

Rename scope had to be re-keyed per language. Phase 2 scoped by executor *family*, which was right when the families were Rust / Python / JS-like. A single `Indexed` family covering four languages would have had a Zig `deploy` block a rename of a Bash `deploy` — the exact Python-vs-JavaScript defect phase 2 fixed, reintroduced by the grouping rather than by the logic. Each language in the indexed and structural tiers is now its own rename family; only the JS-like executors share one, because they genuinely call each other.

Zig and GDScript recognize `rename_symbol` and **not** `structural_rewrite` — this build compiles no ast-grep grammar for them, and advertising a kind that can only fail at match time is worse than declining it at registration.

**I need to correct the plan's premise for `#tsrenameast4`.** It said the structural-only `rename_symbol` refusal would become reachable end to end once an indexed language could otherwise fall through. It does not. Making *every* indexed language renamable leaves the structural-only tier defined by having no `tsift-graph` binding at all, so there is no symbol to resolve and the index layer answers first:

    $ echo '{"intents":[{"kind":"rename_symbol","symbol":"widgetCount","file":"main.go",...}]}' | tsift edit-intents
    Error: no indexed symbol matched "widgetCount"

Checked live against a Go fixture, not reasoned about. Phase 3 dissolved the premise instead of satisfying it.

What phase 3 *did* make reachable is the same guard through a different door. Zig and GDScript are the first executors that recognize one kind and refuse another, so `structural_rewrite` on a `.zig` file now reaches the executor-level refusal end to end and names the executor. Markdown does the same for `rename_symbol`. Both have end-to-end tests, and the structural-only case has one pinning where its refusal actually comes from, so a later change that moves it reads as a change rather than as a regression. I amended the plan file rather than leave the overstated claim standing.

**Conformance.** Two exhaustive tables now, keyed on the recognized-intent set rather than on a hand-kept list: a structural fixture per executor that recognizes `structural_rewrite`, a rename fixture per executor that recognizes `rename_symbol`. A `Lang` brought into the renamable tier with no rename row fails the suite. Each rename row declares the positions that must change *and* the positions that must survive byte for byte, asserted individually.

I mutation-checked the ones that matter, since a rename table is easy to write so that it proves nothing:

| mutation | reddens |
|---|---|
| drop the bash `word` position rule | bash row — `echo widget_count` gets renamed |
| add `string_content` to Zig's identifier set | zig row — the string literal gets renamed |
| drop `"name"` from GDScript's set | gdscript row — the declaration is not renamed |
| collapse the indexed tier to one rename family | the family-separation test |
| let Zig advertise `structural_rewrite` | 4 tests, including the ast-grep grammar guard |
| remove the indexed dispatch branch | the end-to-end apply test |

One pre-existing test asserted the old behaviour — `edit_intents_structural_rewrite_reaches_the_kotlin_and_bash_executors` checked that Kotlin and Bash *refuse* `rename_symbol`. It now checks `replace_function_body` instead, which is a kind that tier still genuinely has no rewriting for.

Live, on the installed binary, `deploy` → `launch` across all four files:

    util.sh    launch() {           <- renamed
                 echo deploy        <- preserved (argument, not a name)
                 local label="deploy"   <- preserved
                 # deploy comment   <- preserved
               launch               <- renamed (call)
    main.zig   pub fn launch()      <- renamed, comment and string preserved
    player.gd  func launch():       <- renamed, comment and string preserved
    caller.gd  return launch() + 1  <- cross-file, no GDScript-specific code

Spec updated: `specs/ast-edits.md` gains an "Indexed executors" section with the tier table and the bash position rule, `specs/structural-patterns.md` corrects "promotion unlocks the symbol-resolved kinds" to "promotion unlocks `rename_symbol` immediately", and `specs/search-navigation.md` lists the new apply-capable languages.

CI run `30589897064` is in progress; the prior run on `784a6cf` was green. Local verification is the evidence above.

Remaining in the queue: `#wsdq` release + publish + install, and `#jzs7` extract-method.

### Re: do [#narrowidentifierkind] — opus-5

Landed. tsift `e22c34a`, gitlink `c806e3928`, pushed. `make check` exit 0, 1654 tests, clippy clean. CI on the previous commit came back green; this one is queued.

The diagnosis I filed this under was half wrong, and the correction is the useful part. I wrote that it "needs per-occurrence scope resolution" and pointed at phase 2's graph scoping. Phase 2 resolves *which files* a rename touches — it has nothing to say about which occurrences inside one file. What this actually needed was the resolved symbol's **kind**, which the planner already had and was discarding before it ever reached the occurrence walk. No new resolution machinery, just stopping throwing away what was in hand.

A Rust function rename now skips `struct S { count: usize }`, `S { count: 1 }`, and `x.count` while keeping `x.count()`. A GDScript function rename skips `var count` and parameter bindings; a variable rename skips `func count`.

    pub struct Meter { pub count: usize }      <- untouched
    pub fn tally() -> usize { 3 }              <- renamed
    impl Meter {
        pub fn tally(&self) -> usize { self.count }   <- decl renamed, field read untouched
    }
    pub fn total(m: &Meter) -> usize { m.tally() + m.count + tally() }
                                       ^^^^^^^^^   ^^^^^^^^^
                                       method call  field read

**The part I did not expect, and would have shipped wrong if I had trusted the green suite.** I ran the installed binary on a GDScript fixture and read the output instead of only the test results. Narrowing alone produced this:

    func tally():
        var count = 1     <- declaration keeps the old name
        return tally      <- its read got the new one

That is incoherent in a way neither the old behaviour (rename both) nor a perfect fix (rename neither) is — the local now reads the function. The tests were green because the fixture had no such read. So the narrowing gained a refusal: when a *dropped* declaration shadows the target and an unattributable reference to it survives, the rename refuses and names the shadow's line rather than emitting that file. A callee is never ambiguous — `count()` is the function whatever is in scope — so a shadow that is only ever called still renames cleanly. Live:

    shadow.gd  →  unsupported | rename_symbol refuses "count": a same-named
                  declaration on line 2 shadows it, and a bare reference cannot
                  say which one it belongs to        (file unchanged)
    clean.gd   →  func tally():  /  var count = 1  /  return tally()

Rust needs none of that: a field and a function are reached through different syntax, so no reference between them is ambiguous.

**Two limits held deliberately.**

Only positions the grammar makes unambiguous get dropped. Under-renaming is the worse failure — it leaves a caller pointing at a name that no longer exists *and reports success*, which is exactly what the cross-file phase existed to stop.

An unresolved or unrecognized symbol kind narrows nothing. A capture name added to `Lang::symbol_query` without a decision about what it is falls through to permissive, so a new symbol kind can never silently start dropping occurrences.

And one limitation is now pinned rather than folklore: Rust macro bodies are exempt by construction. `m.count` inside `format!` is a bare `identifier` with no `field_expression` around it, because tree-sitter parses macro arguments as an opaque `token_tree` — the same opacity that lets the walk reach real call sites inside `assert_eq!`. It is still renamed, and there is a test saying so.

Mutation-checked in both directions, since a narrowing test can pass by narrowing too much just as easily as too little:

| mutation | reddens |
|---|---|
| narrowing disabled entirely | 6 unit rows + the conformance table |
| Rust accepts `field_identifier` unconditionally | 3 unit rows + the table |
| Rust drops `field_identifier` entirely (method calls lost) | the method-call row |
| GDScript accepts `name` for any declaration parent | 2 unit rows + the table |
| shadow refusal removed | both shadow rows |
| a callee counted as ambiguous (over-refusal) | the clean-shadow row |

**A process note, since it cost real time.** I clobbered the 28-row structural fixture table with a scripted string splice whose anchor (`executor: ...::Rust, alias: "rust"`) appears in *two* tables, and the edit silently deleted everything between them. It compiled far enough to look like an unrelated error. Recovered by extracting the region from `HEAD` and re-applying the intended change against a unique anchor. This is the "use the AST, not text matching, for source edits" rule earning its keep on a repo whose own tool exists to do exactly that.

Follow-up filed at the top of the backlog: the JS-like family has the same shape and is genuinely harder, because `{ foo }` shorthand is both a key and a read — no position rule can be right there, so it probably wants the shadow refusal rather than narrowing.

### Re: do [#applysamesymbol] — opus-5

Landed. tsift `adb5c56`, gitlink `ae84e4ef6`, pushed. `make check` exit 0, 1659 tests, clippy clean. Last cycle's CI came back green; this one is in progress.

The narrowing now covers TypeScript, TSX, JavaScript, and JSX. `property_identifier` spans an object-literal key, a class method, and a member access, and none of them is the module-level binding a rename resolves to — the symbol query indexes `function_declaration`, `class_declaration`, and arrow-valued `variable_declarator` and nothing else — so all three are dropped. `type_identifier` is kept only for a type rename.

**The prediction in the backlog item was wrong, and being wrong is what made this a fix instead of a refusal.** I filed it saying `{ beta }` shorthand is "genuinely undecidable... likely needs the shadow refusal." It is not undecidable. The token is both the property name and a read of the binding, and there is a spelling where both stay correct — the one the shorthand already desugars to:

    function gamma(v) { return v; }              <- renamed
    const keyed = { beta: 1 };                   <- key untouched
    const shorthand = { beta: gamma };           <- expanded, not overwritten
    class Panel { beta() { return 2; } }         <- method untouched
    const total = keyed.beta + gamma(3);         <- member read untouched, call renamed

That is the installed binary on a real file, not a fixture. Refusing there would have blocked an ordinary re-export, which is a worse outcome than either thing I was choosing between. `IdentifierOccurrence` now carries the expansion as a flag so the splice decides the spelling, rather than every caller having to.

**One case is deliberately left alone.** `const { beta } = mod` looks like the same shorthand but is not: the token reads a property off `mod` *and* declares a local of the same name, so the right rewrite depends on whether `mod` is the module whose export was renamed. That is the common case, plain span renaming already handles it correctly, and expanding it would break exactly the shape that works today. There is a test asserting it is renamed in place and not expanded.

Mutation-checked in both directions, since a narrowing can be wrong by dropping too much as easily as too little:

| mutation | reddens |
|---|---|
| `property_identifier` accepted again | 2 unit rows + conformance table + E2E |
| shorthand expansion disabled (plain overwrite) | 1 unit row + table + E2E |
| destructuring pattern expanded too | the in-place row |
| `type_identifier` accepted for callables | the type-rename row |

The JS-like conformance fixtures now carry an object key, a class method, and a member access each, so the table catches this class rather than only the unit tests.

Remaining in the queue: `#wsdq` release + publish + install, and `#jzs7` extract-method. I filed one follow-up above it — Python and Kotlin are the two indexed languages still on a flat identifier set, and they have the same shape through different nodes (`identifier` under `attribute` / `navigation_expression`). They need one twist the others did not: both index methods as functions, so the callee position is a genuine target and must be kept rather than dropped.
<!-- agent:boundary:b332ad5b -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- do [#finishsymbolkind]
- ~~do [#applysamesymbol]~~
- ~~do [#narrowidentifierkind]~~
- do [#wsdq]
- do [#jzs7]
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#finishsymbolkind] Finish the symbol-kind narrowing for Python and Kotlin, the two indexed languages still at a flat identifier set. Both have the Rust field_identifier shape via a different node: Python spells obj.foo as an identifier under attribute, Kotlin as an identifier under navigation_expression, so renaming a module-level def/fun still rewrites every same-named attribute access. The rule is the one already used three times — accept the position only when it is the callee of a call — but unlike Rust, Python and Kotlin index methods as functions too, so the callee case is a real target and must be kept. Plan: tasks/software/plan-tsift-rename-symbol-ast.md
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
