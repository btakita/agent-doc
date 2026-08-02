# Plan — `extract_function`, the first range-selected edit intent

`#jzs7`. Owning repo: `src/tsift`. Spec home: `specs/ast-edits.md`.

## What makes this different from every intent that exists

Every current semantic edit selects a **named thing** — a symbol row, a Markdown
heading, an ast-grep pattern — and rewrites at or around it. `extract_function`
selects a **range of statements** that has no name, no symbol row, and no single
AST node. `SemanticEditIntent` has no field that can even express it today:
`target_handle` resolves one node, and a run of sibling statements is not one
node.

The second difference is that the output is not a rewrite of what was selected.
It is two edits that must agree with each other — a new function whose signature
is *derived* from the selection, and a call whose arguments are derived from the
same analysis. Get the derivation wrong and the result still parses, still
formats, and does not compile. That is the failure mode this plan is organized
against.

## The derivation, stated once

For a contiguous run of sibling statements `R` inside the body of enclosing
function `F`:

- **Parameters** = names read in `R` before `R` assigns them, that are bound
  somewhere in `F` outside `R` (or are `F`'s own parameters). A name that is
  only ever assigned in `R` is a local of the new function, not a parameter. A
  name bound at module scope is neither — it stays a free reference, exactly as
  it was.
- **Returns** = names assigned in `R` that are read anywhere in `F` after `R`.
  Zero returns emits a bare call; one emits `name = call(...)`; several emit a
  tuple destructuring, which is the only spelling that keeps the call site a
  single statement.
- Everything else is a **refusal**, and the refusal list below is load-bearing.

## Why Python is phase 1 and Rust is not

Python needs no type information to emit a correct signature: `def
extracted(a, b): ...; return c` is complete. Rust cannot be done the same way —
choosing `a: T`, `a: &T`, or `a: &mut T` needs types tsift does not have, and
guessing produces code that parses and does not build. So the family order is
the one the language forces, not the one the codebase's `SEMANTIC_EDIT_*_KINDS`
tables suggest:

1. Python — untyped signature, explicit `return`, indentation-scoped blocks.
2. GDScript, JavaScript, TypeScript-without-annotations — same derivation, and
   TS gets `: unknown` nowhere; if a parameter's type cannot be spelled the
   intent refuses rather than annotating badly.
3. Rust — either behind an explicit caller-supplied signature, or as a closure
   (`let extracted = |a, b| { ... };`), which infers. Deferred deliberately;
   see "What phase 3 has to answer" below.

## Phases

### Phase 1 — `#jzs7extract1` — Python, end to end

Analysis core in `packages/tsift-graph/src/extract.rs`: given source bytes, a
`Lang`, and a line range, resolve the enclosing function, verify the range is a
contiguous run of siblings in exactly one block, and compute parameters and
returns by the derivation above. It returns a plan or a typed refusal — it never
returns a "best effort" signature.

CLI wiring in `packages/tsift-cli/src/semantic_edit.rs`: a new `extract_function`
kind with `start_line`/`end_line` on `SemanticEditIntent`, registered for the
Python executor only, dispatched ahead of the symbol-resolved families the way
`structural_rewrite` already is, since there is no symbol to resolve.

Exit criteria: a fixture extracts a two-statement range that reads two outer
locals and produces one value, asserting the emitted `def`, its parameter list,
its `return`, and the rewritten call site **individually**. A second fixture
asserts each refusal below fires with its own message.

### Phase 2 — `#jzs7extract2` — the untyped family

GDScript and the JS-like grammars, reusing the phase-1 core with a per-language
emitter for the `def`/`function`/`func` spelling and the indentation or brace
block. Registration is a table entry plus a conformance fixture row, the same
shape phase 3 of the rename plan settled on.

### Phase 3 — `#jzs7extract3` — Rust, and what it has to answer

Not "implement Rust". First answer: does the caller supply the signature, or do
we emit a closure? A closure is correct without types but changes borrow
behaviour and cannot be `pub`. A caller-supplied signature is honest but moves
the hard part to the caller. Pick one with a written rationale before writing
code, because this is the decision that determines whether the intent is useful
or merely available.

## Refusals — the list is the feature

Each of these produces a typed refusal naming the invariant, never a patch:

- The range is not a contiguous run of siblings in one block (it straddles an
  `if`/`else` boundary, starts mid-statement, or spans two functions).
- The range is not inside a function at all.
- Control flow escapes the range: `return`, `break`, `continue`, `yield`, or a
  bare `raise` that the enclosing function catches. Hoisting any of these
  changes what the statement does, and no signature can carry it.
- The range assigns to a name declared `global` or `nonlocal` in `F`.
- More than one name is assigned in `R` and read after it *and* the range also
  produces a control-flow effect — the tuple return cannot carry both.
- The enclosing function's body has a parse error overlapping the range.
- The new name already binds something in `F`'s scope or at module scope.

Under-refusing here is worse than under-refusing in a rename: a rename that
misses an occurrence breaks the build loudly, but an extraction with a wrong
parameter list can *compile and silently change behaviour* — a name that should
have been a parameter falling through to a module-level binding of the same name
is the exact case, and it is why module-scope names are classified explicitly
above rather than left to "not found in `F`".

## Status

Phase 1 landed in `#jzs7`. Phase 2 landed in `#registerextractfunction`
(tsift `e272087`): Python, GDScript, JavaScript, JSX, TypeScript, and TSX share
one analysis, with the node-kind vocabulary and the emitter as `Dialect` data
rather than inlined Python kinds. Phase 3 is `#deciderustspells` — still a
decision, not an implementation.

Three things the generalization turned up that phase 1 had wrong, each of which
emitted code that parsed:

- A bare assignment does not declare outside Python, so a range that only
  *assigns* a name declared behind it left the new function writing a name that
  was not in scope. The new function now opens with those declarations, and
  refuses when the name is not a local of the enclosing function at all.
- `obj.attr = 1` counted the receiver as a binding, so it stopped being a
  parameter.
- `base = base * 2` recorded its write at the target's own offset, so the
  right-hand read looked like a read of an already-assigned local.

Two refusals were added beyond the list above: an enclosing function that sits
in an *expression* rather than a statement (an arrow function, a class
expression — there is no statement slot beside it), and, for GDScript, more
than one returned name, since it has no destructuring call site.

`#extractfrommethod` (tsift `d98c2df`) then turned the method case from a
refusal into the answer to a different question — not "can this be extracted"
but "where does the new function go so the bare call still resolves". Python
climbs past the class to module scope with `self` threaded through the
signature first; JS/TS land beside the class declaration and refuse a range
that names `this`; GDScript deliberately stays inside the class, because its
methods call each other bare. The climb stops at the first non-class statement
position, which keeps a nested function's extraction inside its enclosing
function and its closure intact.
