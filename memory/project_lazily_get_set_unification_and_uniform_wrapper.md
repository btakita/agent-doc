---
name: project_lazily_get_set_unification_and_uniform_wrapper
description: "lazily v2 API — unified get/set + single Read/Write<Ctx> trait (rs committed 74295b8), and the LOCKED uniform-wrapper reactive-tracking design (per-call wrapper threads the current-node token as a value; rebind per recompute; browser-safe async)"
metadata:
  node_type: memory
  type: project
  originSessionId: 319d60e9-f1fe-4008-a70c-af4e66292817
  modified: 2026-07-21T23:29:31.623Z
---

Extends the Cell kernel v2 ([[project_lazily_memo_rename_plan_status]]). Decided
interactively 2026-07-21; user drove every call.

**Naming ledger (locked):**
- Value vocab HARD-DELETE (0.x = no stability promise): `Signal`/`signal`/
  `signal_def`, `formula`/`formula_def`, `FormulaCell`, `memo`, `EffectHandle`,
  `drive`/`undrive`/`is_driven`, `is_active`, `SourceCell`/`SourceCellSlot`.
- Construction sugar DEPRECATE-KEEP: `source`/`computed` canonical (fns AND rs
  macros `source!`/`computed!`); `cell`/`slot` deprecated aliases (macros AND
  `ctx.cell()`/`ctx.slot()` methods). Decorators (the macros) stay — user
  affirmed "keep the decorators".
- `slot` is GUARDED ≡ `computed` (deprecated alias). NO unguarded derived
  escape; every cell guarded, non-`PartialEq` derived unsupported.
- `get_cell`/`set_cell` → DEPRECATE toward generic `get`/`set` (~1150 rs sites).

**get/set unification (rs DONE, committed-not-pushed `74295b8`):**
- One `pub trait Read<Ctx: ?Sized> { type Output; fn read }` +
  `Write<Ctx> { type Value; fn write }` in `context.rs`, generic over the
  context type — collapses the 6 per-context read/write trait pairs I first
  wrote (ContextRead/ThreadSafeRead/AsyncRead/AsyncComputeRead/
  TypedThreadSafeRead/RefRead) into ONE. `Output` carries the async
  `Option<T>`-vs-`T` asymmetry. `get<H: Read<Self>>(&self,&H)->..Output`;
  `set<H: Write<Self>>`. Write-protection preserved (set only takes source
  handles → `ctx.set(computed,_)` won't compile). `TypedGet`/`TypedSet`
  (Kind-param, decorator-factory coherence) left as-is, NOT collapsed.
- rs `make check` GREEN with this. Push held (async-tracking discussion).

**Reactive tracking — UNIFORM WRAPPER (LOCKED, NOT yet built anywhere):**
- Every computed call (sync AND async) gets a per-call wrapper ctx that carries
  the current-node id and threads it **as a value** (captured in the closure).
  Reads go `ctx.get(handle)` and attribute to the wrapper's node.
- WHY (the key insight, user-derived): threading the token as a value SURVIVES
  `.await` for free (closures keep their bindings), so post-await/late-binding
  reads attribute correctly with ZERO runtime support — the ONLY mechanism that
  works in **browser JS** (no `AsyncLocalStorage`). Ambient schemes (module var
  / current-on-Context / thread-local) get CLOBBERED across await by other
  interleaved recomputes. `contextvars`/`AsyncLocalStorage`/`Zone` merely
  simulate value-threading; the wrapper does it directly.
- REBIND per recompute (dynamic deps), NOT accumulate — branching correctness:
  `cond ? a() : b()` must DROP the untaken branch's deps. rs already does this
  (collect per-run set + `update_dependencies` diff under a revision guard).
- Cost = 1 alloc/call, POOLABLE to ~0 amortized (rs already pools the deps set:
  `take_deps`/`recycle_deps` — pool the wrapper too).
- Supersedes the abandoned "poll-scoped thread-local / eliminate the wrapper"
  pivot — that premise was WRONG for browser. `AsyncComputeContext` (rs) is the
  RIGHT design, not a wart; keep it. Uniform = sync ALSO gets a wrapper (drops
  rs's sync thread-local), one mechanism everywhere.
- Async-tracking scope if it ever diverges: rs + py(contextvars) + js(wrapper,
  browser-safe) + dart first; go/zig/cpp keep the wrapper regardless.

**Original task still pending:** "release + publish all lazily packages" — far
from done. Next after uniform-wrapper build: fan the ledger out to 6 bindings,
version-bump, publish (crates.io/PyPI/npm/GH/Go-proxy; npm+PyPI need operator
2FA). [[reference_lazily_release_targets]] [[feedback_discuss_naming_before_applying]]
