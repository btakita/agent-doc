# Plan — make `rename_symbol` AST-driven and language-general

Status: proposed, 2026-07-30
Repo: `src/tsift`
Owner surface: `packages/tsift-cli/src/semantic_edit.rs`, `packages/tsift-graph`, `packages/tsift-index`
Backlog: `#tsrenameast1` … `#tsrenameast4` in `tasks/software/tsift.md`

## Why

`rename_symbol` is tsift's flagship symbol-resolved edit kind — the one that
justifies the whole edit-intents planner over `ast-grep rewrite`. It is
implemented as a **substring scan**, not an AST edit:

- `replace_rust_identifier` (`semantic_edit.rs:2047`) walks
  `content.match_indices(old)` and accepts a hit when neither neighbouring
  character is an identifier character. No parse is involved at all.
- `replace_script_identifier` (`semantic_edit.rs:2134`) does the same scan and
  adds a parse before and after. The parse only proves the file still *parses*;
  it cannot notice that the rename landed somewhere it should not have.

Four defects follow directly from that. The first three were reproduced live
against the installed `tsift 0.1.78`.

### 1. Renames inside string literals and comments

Fixture `src/lib.rs`:

```rust
/// Doc comment mentioning widget_count on purpose.
pub fn widget_count() -> usize { 3 }

pub fn describe() -> String {
    // widget_count in a line comment
    let label = "widget_count";
    format!("{label}: {}", widget_count())
}
```

After `rename_symbol widget_count -> gadget_count`, the doc comment, the line
comment, **and the string literal** are all rewritten. The string literal case
is a silent behaviour change: that value could be a serialization key, a config
lookup, a log grep target, a database column. Reported as
`applied=true conflicts=0`.

### 2. No field reports the extent of the edit

The same run printed `range: 2-2` while the change spanned lines 1–9.

**Correction to the first draft of this plan:** that number is not wrong — the
field is `target_range`, the resolved symbol's *declaration* span, and it was
doing its job. The defect is narrower and is a reporting gap rather than a bad
value: there was no field describing what the edit actually touched, and the
human printer labelled the declaration span `range:`, which reads as the extent
of the change. Fixed in phase 1 by relabelling it `target range:` and adding an
`edited_range` computed from the two revisions of the file.

### 3. Rename is per-file, so it breaks every caller

Fixture: `src/lib.rs` defines `widget_count`; `src/caller.rs` does
`use crate::widget_count;` and calls it. After the rename:

```
=== lib.rs ===    pub fn gadget_count() -> usize { ... }
=== caller.rs === use crate::widget_count;
                  pub fn total() -> usize { widget_count() + 1 }
```

Reported as `planned=1 applied=1 conflicts=0`. The tree does not compile, and
nothing in the report says so. This is the defect that matters most: tsift
**already has the call graph** that knows exactly which files reference the
symbol, and the rename does not consult it.

### 4. It doesn't generalize

Because each family hand-rolls its own scan, `rename_symbol` exists for Rust,
Python, and the JS-like family only. Every other indexed language — bash, zig,
and now gdscript — is refused, and each new language would mean another copy of
the same wrong scan. The refusal for *structural-only* languages is correct and
should stay; the refusal for *indexed* languages is an artifact of the
implementation, not a real constraint.

## Approach

Replace the per-family text scan with one AST-driven implementation that reads
identifier occurrences out of the tree-sitter tree, and use the existing call
graph to scope the rename across files.

The key observation: **the graph tier already computes what the rename needs.**
`Lang::symbol_query()` finds definitions, `Lang::call_query()` finds call sites,
and the index stores byte spans for both. A rename is a span rewrite over those
spans plus the language's identifier nodes — none of which needs new
per-language prose, only a per-language *identifier node kind* set, which is one
small table.

### Phase 1 — `#tsrenameast1` — AST occurrence collection (single file) — **DONE**

Add `graph::rename::identifier_occurrences(lang, source, name) -> Vec<Span>`:
walk the tree and yield only nodes whose kind is in that language's
identifier-node set and whose text equals `name`. Comments and string literals
are *different node kinds*, so they drop out by construction rather than by a
special case.

Rewire `replace_rust_identifier` and `replace_script_identifier` to consume it,
deleting both scans. Report the true range from `min(span.start)` to
`max(span.end)`, which fixes defect 2 as a side effect.

Exit criteria: the defect-1 fixture renames the definition and the call site and
leaves the doc comment, line comment, and string literal untouched. The
regression test asserts each of those positions **individually** — a test that
only counts replacements would pass on the wrong ones.

**Landed.** `packages/tsift-graph/src/rename.rs` holds the walk and the
per-language identifier-kind table; both scans in `semantic_edit.rs` now route
through it. One risk was checked before committing to the design: tree-sitter
parses Rust macro arguments as an opaque `token_tree`, so an identifier-node
walk could have *lost* call sites inside `assert_eq!`/`format!` that the old
substring scan found. A probe of the parse tree showed identifiers inside a
`token_tree` are still named `identifier` nodes, so there is no regression — and
that case now has its own test.

Known limitation carried into phase 2: the Rust identifier set includes
`field_identifier`, so renaming a function also renames an identically-named
struct field. That matches the old scan's behaviour; narrowing it needs the
graph-scoped resolution phase 2 introduces.

### Phase 2 — `#tsrenameast2` — cross-file rename via the call graph

Resolve the symbol through the index, collect its referencing files from the
call-edge table, and emit one patch per file within a single intent, so the
whole rename lands or none of it does (the batch-rollback path already exists).

Where the graph cannot prove a reference is the same symbol — a same-named
method on an unrelated type, dynamic dispatch — the intent must **refuse rather
than guess**, and name the ambiguous site. A rename that silently renames the
wrong `add()` is worse than one that declines.

Exit criteria: the defect-3 fixture renames both files and the result compiles.
A second fixture with a same-named-but-unrelated symbol in a third file is
refused with that file cited.

### Phase 3 — `#tsrenameast3` — open it to every indexed language

With the implementation language-general, registration becomes a per-language
identifier-node-set entry plus a fixture row. Bring bash, zig, and gdscript into
the recognized set for `rename_symbol`.

Markdown is deliberately out: it already has `rename_heading`, which is the
right kind for it.

Structural-only languages stay refused. That refusal is correct and is exactly
the guard described at `tasks/software/tsift.md:337` — indexing buys navigation,
and rewriting still needs a resolvable symbol.

Exit criteria: extend the cross-language conformance suite so a `Lang` variant
that claims `rename_symbol` with no rename fixture row is a failure, the same
way `packages/tsift-graph/tests/conformance.rs` already treats symbol
extraction. Fixtures assert renamed *and* untouched positions per language.

### Phase 4 — `#tsrenameast4` — retire the dead guard

`tasks/software/tsift.md:358` records an honest gap: the executor-level refusal
of `rename_symbol` for structural-only languages is asserted in unit tests but
is unreachable end to end, because an unindexed language has no symbol to
resolve and the index layer refuses first. After phase 3 that guard becomes
reachable for the first time (gdscript is indexed and would otherwise fall
through), so add the end-to-end test that was impossible to write before.

## Sequencing

Phases 1 and 2 are independently landable and each fixes a live correctness
defect on its own. Phase 3 is additive and depends on 1. Phase 4 depends on 3.

Phase 1 is the highest value per unit of work: it is the one that stops tsift
from silently editing string literals.

## Non-goals

- No LSP or type-resolution dependency. The call graph is the resolution
  substrate; where it is not conclusive, the rename refuses.
- No rename across language boundaries (a Rust symbol named inside a `.md`
  fence).
- No change to the planner contract: patch proposal, bounded diff preview,
  `expected_content_hash` conflict detection, `--verify` in a detached temp
  worktree, and batch rollback all stay exactly as they are.
