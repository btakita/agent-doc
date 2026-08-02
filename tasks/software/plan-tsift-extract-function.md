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
3. Rust — deferred deliberately, and answered in phase 3 by neither of the two
   options this line originally listed: it copies the annotation that already
   exists and refuses where there is none, extracting by value only.

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

### Phase 3 — `#jzs7extract3` — Rust: answered, then implemented

The question was: caller-supplied signature, or a closure? **Neither.** The
answer is a third option the framing had folded into the first — copy the
annotation that already exists, and refuse where there is none.

The ownership qualifier and the type turned out to be separable questions with
different answers. The qualifier is derivable from the use analysis the intent
already does. The type is not, and nothing short of a type checker gets it:
`let` bindings are idiomatically unannotated, and inferring one needs name
resolution, trait resolution, and method return types. tsift has no type
information of any kind.

The qualifier does not rescue the rest either. A `&T` or `&mut T` parameter
requires rewriting each use of the name in the body into a dereference —
`acc += 1` becoming `*acc += 1` — and rewriting bodies is what this intent does
not do. So **Rust extracts by value**, refuses a name that would be moved in
while the caller still reads it, and *threads* a name the range both reads and
reassigns: moved in and handed back as part of the return, which covers the
accumulator shape with no borrow at all.

Why not a closure: it cannot be `pub`, cannot move to another module, cannot be
documented or tested on its own, changes borrow behaviour through what it
captures, and would succeed exactly where the free function honestly declines.
Why not a caller-supplied signature: it gives up the one property that makes
this intent worth having — that the signature and the call site come from one
analysis and cannot disagree.

## Refusals — the list is the feature

Each of these produces a typed refusal naming the invariant, never a patch:

- The range is not a contiguous run of siblings in one block (it straddles an
  `if`/`else` boundary, starts mid-statement, or spans two functions).
- The range is not inside a function at all.
- Control flow escapes the range: `return` or `yield` always; `break` and
  `continue` only when the loop — or, in JS, the `switch` — they bind to stays
  behind, and a labelled branch only when its label does. A range holding a
  whole loop takes that loop's `break` with it and nothing changes. (Narrowed
  in `#extractloopbreak`, tsift `2ddd73c`; `raise`/`throw` is not on the list
  at all, since an exception propagates through a call frame unchanged.)
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

All three phases have landed. Phase 1 in `#jzs7`. Phase 2 landed in `#registerextractfunction`
(tsift `e272087`): Python, GDScript, JavaScript, JSX, TypeScript, and TSX share
one analysis, with the node-kind vocabulary and the emitter as `Dialect` data
rather than inlined Python kinds. Phase 3 landed in `#deciderustspells`
(tsift `ec381ab`): Rust joins them by value, copying the types it can already
see. Registered languages are now Python, GDScript, JavaScript, JSX,
TypeScript, TSX, and Rust — one analysis, seven vocabularies and emitters.

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

`#extractstalerange` added the one refusal that is a property of the *batch*
rather than of the range. Intents apply in order against one shared buffer, and
every named intent re-resolves itself against it; a line number cannot, so a
range-selected intent whose file an earlier intent already edits is addressing
lines that have moved. It refuses at plan time by name, and the refusal blocks
the whole batch so the earlier intent does not land either. Scoped to the file,
not to the kind — an inserted import shifts lines exactly as an extraction does.

`#extractfrommethod` (tsift `d98c2df`) then turned the method case from a
refusal into the answer to a different question — not "can this be extracted"
but "where does the new function go so the bare call still resolves". Python
climbs past the class to module scope with `self` threaded through the
signature first; JS/TS land beside the class declaration and refuse a range
that names `this`; GDScript deliberately stays inside the class, because its
methods call each other bare. The climb stops at the first non-class statement
position, which keeps a nested function's extraction inside its enclosing
function and its closure intact.
