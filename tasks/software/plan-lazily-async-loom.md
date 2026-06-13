# Plan: deterministic #k03k resolve-loop window coverage (#lzasyncloom) — DONE

## Outcome

`#ytp5`/`#lzasyncloom` was filed as "add a **Loom** model for the async resolve
loop." Investigation showed that framing is infeasible: **Loom cannot model
`async_context.rs`.** Loom only shims synchronous `loom::sync` primitives and has
no async executor; `AsyncContext` runs on tokio's executor + `tokio::sync::watch`.
(`thread_safe.rs` has a Loom model precisely because it was *designed* around
swappable atomics — `#[cfg(loom)] use loom::sync` at the top.)

So the item was re-scoped to its actual goal — **deterministic regression
coverage of the two `#k03k` resolve-loop windows** — and delivered with the tool
that fits the tokio async path.

## What shipped

- `tests/async_resolve_loop.rs` (`async` + `instrumentation` features):
  - **Window 1** (`Resolved` published in the synchronous gap between the
    fast-path `get()` and the re-lock, `async_context.rs:773-790`): forced
    deterministically via a one-shot `instrumentation`-gated seam
    (`AsyncContext::__install_window1_hook`) that resolves the slot inside the
    gap. The gap has no `.await`, so cooperative scheduling cannot reach it — the
    seam is required. The test asserts the reader returned through the
    `Resolved`-after-re-lock arm via `__window1_resolved_hits`.
  - **Window 2** (notifier dropped on supersede → re-resolve): gates an in-flight
    compute, supersedes it with a newer revision, asserts the waiter re-resolves
    to the latest value rather than panicking.
- Production seam in `src/async_context.rs`, entirely behind
  `#[cfg(feature = "instrumentation")]` (compiled out of default/release builds):
  `Window1Hook` type, `window1_hook` one-shot field, `window1_resolved_hits`
  counter, `__install_window1_hook` / `__window1_resolved_hits` accessors, and the
  gap fire-point + Resolved-arm counter.
- `SPEC.md` `get_async` re-resolve contract: documents the Loom-incompatibility,
  the deterministic window coverage, and that exhaustive interleaving would need
  Shuttle.
- `Makefile`: `test-async-resolve` target, added to `check` and `.PHONY`.
- CI (`.github/workflows/ci.yml`): added **Test async feature** (async tests were
  not run in CI at all — `async` is not a default feature) and **Test async
  resolve-loop windows (#k03k)** steps, alongside the existing loom step.

## Follow-up (operator-gated): #lzasyncshuttle

Exhaustive interleaving exploration of the async path (beyond the two known
windows) would require a **Shuttle** model. That needs `async_context.rs` to be
generic over its concurrency primitives (swap tokio↔shuttle executor + `watch` +
`Arc` under `cfg`), the way `thread_safe.rs` is generic over atomics for Loom.
That is a meaningful architectural change whose primary beneficiary is the test
harness, and the shimmed path it verifies is an approximation of tokio rather than
the real path. Recommended only if the async graph grows more concurrency-critical
paths — queued as `#lzasyncshuttle`, not bundled here.
