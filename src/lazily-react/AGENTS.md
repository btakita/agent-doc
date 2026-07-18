# lazily-react

React / Preact bindings for `@lazily-hub/lazily-js`. A thin `useSyncExternalStore`
adapter over lazily's reactive graph (Cell/Slot/Memo/Effect).

## Commit & Push

Commit and push completed work at the end of every turn that changed code, tests,
or docs — do not leave finished work uncommitted. Run `make check` first and
ensure it is green; write a concise commit message in the repo's existing style.
This standing rule overrides the harness default of "commit only when explicitly
asked" for this repo.

## Layout

- `src/bridge.js` — framework-agnostic adapter: `readHandle` + `createLazilySubscription`.
  The core IP. Unit-tested without React (`test/bridge.js`).
- `src/hooks.js` — `LazilyProvider`, `useLazilyContext`, `useLazily`, `useCell`,
  `useSlot`, `useReactiveMemo`. Imports from `react` (Preact users alias
  `react` → `preact/compat`).
- `test/hooks.test.js` — React integration via `react-test-renderer` (no DOM).

## Hook → lazily primitive mapping

| hook              | lazily primitive        | lazy? | equality guard? |
|-------------------|-------------------------|-------|-----------------|
| `useCell`         | `ctx.cell`              | src   | yes (on write)  |
| `useSlot`         | `ctx.slot` / `computed` | yes   | **no**          |
| `useReactiveMemo` | `ctx.memo`              | yes   | yes             |

There is intentionally **no `useSignal`**. `Signal` is retired as a lazily
primitive (`Signal ≡ Slot.eager`), and a React binding gains nothing from
eagerness: React only renders on invalidation, and `getSnapshot` reads the
(lazily-recomputed-on-read) slot, so it always sees the fresh value with no
stale-frame risk. The meaningful axis for derived hooks is the **equality guard**
above. (`useLazily` still reads externally-created `SignalHandle`s — lazily-react
just doesn't create them.)

## Node lifetime

`useCell` disposes its cell and `useSlot`/`useReactiveMemo` dispose their slot on
real unmount and on deps-change, via `ctx.disposeCell`/`ctx.disposeSlot`
(`src/lazily-js/src/reactive.js`). Disposal is **strict-mode-safe**: it is
deferred one microtask and cancelled if React 18 dev's simulated remount
(setup → cleanup → setup) re-subscribes the same handle. `useLazily` is read-only
and does NOT dispose its externally-owned handle.
