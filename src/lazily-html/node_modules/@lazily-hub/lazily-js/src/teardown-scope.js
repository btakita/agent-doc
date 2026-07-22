// Teardown scopes for the synchronous reactive `Context` (`#lzspecedgeindex`).
//
// Its own module, not a section of `./reactive.js`, for a measured reason: the
// class carries a computed member key (`[Symbol.dispose]`), and esbuild treats a
// computed class key as making the class definition impure — so the class
// survives tree-shaking in any bundle that imports anything at all from its
// module. Sitting in `reactive.js` it added 185 B to every such consumer,
// including `state-machine.js`, which imports a single handle class and will
// never open a scope. Here, `"sideEffects": false` lets the bundler drop the
// whole file for them.
//
// `reactive.js` re-exports it, so the public API is exactly as before.

// The TC39 explicit-resource-management key, with the standard polyfill fallback
// (`Symbol.for("Symbol.dispose")` is the well-known key core-js and TypeScript's
// downlevel emit both use) on engines that predate it.
//
// A module-level `const` + a computed class member rather than an
// `if (...) Scope.prototype[Symbol.dispose] = ...` assignment after the class:
// that form is a module-level SIDE EFFECT, which defeats `"sideEffects": false`
// tree-shaking and drags `TeardownScope` into every bundle that imports anything
// from this module. It measurably did — it added 183 B to the `state-machine`
// entry, which uses no scopes at all.
const DISPOSE = Symbol.dispose ?? Symbol.for("Symbol.dispose");

/**
 * A teardown scope over a {@link Context}: nodes created through it are disposed
 * when it ends (`#lzspecedgeindex`).
 *
 * ## Why the end is explicit
 *
 * `lazily-rs` ends a scope in `Drop`, so the scope's lifetime is the block's and
 * there is nothing to forget. JavaScript has no destructor. The only GC-driven
 * hook is `FinalizationRegistry`, which the language explicitly does not
 * guarantee to run at all — so a scope that tore down on collection would
 * reintroduce, non-deterministically and in production only, exactly the
 * unbounded-dependent-list leak scopes exist to prevent. That shape is strictly
 * worse than no scope, and it is not offered.
 *
 * The end is therefore a statement, in the three shapes JavaScript callers
 * actually need:
 *
 * - `ctx.withScope(body)` brackets the scope and ends it in a `finally`. The
 *   direct analogue of the Rust block scope, and the default.
 * - `using scope = ctx.scope()` — the scope implements `Symbol.dispose`, so
 *   TC39 explicit resource management ends it at the end of the enclosing block
 *   on runtimes that support the syntax. This is the closest JavaScript has to
 *   `Drop`, and it is lexical and static, not GC-driven, which is precisely why
 *   it is safe to offer where a finalizer is not. It is additive: the symbol is
 *   only installed when the runtime defines it, so nothing breaks on older ones.
 * - `scope.end()` covers what neither bracket can express — a scope whose
 *   lifetime is a *connection*, a *subscription*, or a *route*: opened in one
 *   callback and ended in another, across an asynchronous gap. That is the
 *   primary use of scopes, so it cannot be bracket-only.
 *
 * All three are idempotent and compose: a scope ended early inside `withScope`
 * is not ended twice.
 *
 * ## What it stores
 *
 * Handles, in creation order. Teardown walks them in REVERSE creation order —
 * dependents before what they read — so the scope never transiently dangles
 * inside itself while tearing down. Graph state is order-independent
 * (`disposeAll_order_independent` in lazily-formal), but effect *cleanups* are
 * side effects and their order is observable; ending a scope is proved
 * observationally equal to disposing each member individually
 * (`disposeScope_eq_disposeAll`).
 *
 * Ending a scope tears its nodes down even if something outside still reads
 * them, and that outside reader then throws {@link DisposedNodeError} on its
 * next recompute — same caveat as disposing a node directly, and the subject of
 * the corpus's `cross_scope_teardown_hazard`.
 */
export class TeardownScope {
  #ctx;
  #owned = [];
  #ended = false;

  /** @internal — obtain one from {@link Context.scope} / {@link Context.withScope}. */
  constructor(ctx) {
    this.#ctx = ctx;
  }

  /** How many nodes this scope currently owns. */
  get size() {
    return this.#owned.length;
  }

  /** Whether {@link end} has already run. */
  get ended() {
    return this.#ended;
  }

  /**
   * Take ownership of an existing node so this scope disposes it at
   * end-of-life. The factories below are the ordinary path; this exists for
   * nodes built by a helper that knows nothing about scopes.
   *
   * A node adopted twice is disposed once (disposal is idempotent). Adopting
   * into an already-ended scope is a no-op rather than an immediate disposal —
   * the scope's moment has passed.
   */
  adopt(handle) {
    if (!this.#ended) {
      this.#owned.push(handle);
    }
    return handle;
  }

  /** Create a source cell owned by this scope (#lzcellkernel). */
  source(value, policy) {
    return this.adopt(this.#ctx.source(value, policy));
  }

  /** Create a guarded computed owned by this scope (#lzcellkernel). */
  computed(compute) {
    return this.adopt(this.#ctx.computed(compute));
  }

  /** Create a guarded computed with a custom propagate predicate, owned by this scope (#lzcellkernel). */
  computedRippleWhen(compute, changed) {
    return this.adopt(this.#ctx.computedRippleWhen(compute, changed));
  }

  /** @deprecated use {@link source}. */
  cell(value) {
    return this.adopt(this.#ctx.source(value));
  }

  /** @deprecated use {@link computed} (guarded, the only derived construction). */
  slot(compute) {
    return this.adopt(this.#ctx.computed(compute));
  }

  /** @deprecated use {@link computed}`(f).eager()` for an eager signal. */
  signal(compute) {
    return this.adopt(this.#ctx.signal(compute));
  }

  /** Register an effect owned by this scope. */
  effect(run) {
    return this.adopt(this.#ctx.effect(run));
  }

  /**
   * Cancel this scope's teardown: ending it afterwards disposes nothing, and its
   * nodes revert to plain context ownership — the state every unscoped node is
   * already in.
   *
   * The nodes themselves are untouched. They keep their values, keep their edges
   * in both directions, keep propagating, and remain individually disposable.
   * The only thing that changes is whether this scope fires at end-of-life,
   * which is what the name says — the same sense as defusing a guard.
   */
  disarm() {
    this.#owned.length = 0;
  }

  /**
   * Dispose every node this scope owns, in reverse creation order. Idempotent.
   */
  end() {
    if (this.#ended) {
      return;
    }
    this.#ended = true;
    const owned = this.#owned;
    // Reverse: dependents before what they read. See the class comment — graph
    // state does not depend on this order, but effect cleanups are side effects
    // and their order is observable.
    for (let i = owned.length - 1; i >= 0; i--) {
      this.#ctx.disposeNode(owned[i]);
    }
    owned.length = 0;
  }

  /** TC39 explicit resource management: `using scope = ctx.scope()`. */
  [DISPOSE]() {
    this.end();
  }
}

