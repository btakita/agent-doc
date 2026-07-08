# Plan — isolate cdylib panics so a Rust bug can never abort the host editor

**Pending:** `#cdylibpanic`
**Status:** Draft (needs its own focused cycle + install)

## Problem

On 2026-07-02 a single parser panic (`❯` UTF-8 byte-boundary slice, since fixed
by `bounded_preview` — see [`plan-component-parser-utf8-boundaries.md`](plan-component-parser-utf8-boundaries.md))
crashed the **entire** IntelliJ IDEA process, not just the failing operation.

Root cause of the *amplification*:

- `Cargo.toml:279` sets `panic = "abort"` on `[profile.release]`, and
  `[profile.release-local]` inherits it. The JetBrains plugin loads the cdylib
  built from `release-local`, so the loaded native library aborts the process on
  **any** panic, on **any** thread.
- There is **zero `catch_unwind`** in the crate. FFI entry points (`ffi.rs`) and
  the callbacks agent-doc registers with the `lazily` CRDT runtime (which runs
  them on its own `agent-doc-crdt-*` threads) have no panic barrier.
- Coredump evidence (pid 2674606): `SIGABRT` on thread `agent-doc-crdt-*`, stack
  entirely inside `libagent_doc.so`, i.e. a Rust panic → `abort()` → JVM death.

Net: any panic reachable from the editor — parser edge case, unwrap, index
out-of-range, a `lazily` callback — takes down the user's IDE.

## Desired behavior

1. A Rust panic originating in the cdylib must **never** abort the host process.
   It should be caught at the boundary, logged, and surfaced as an error/no-op so
   the editor keeps running.
2. The CLI binary may keep whatever panic strategy is best for it; the **cdylib**
   must be built to unwind (with a boundary that never lets a panic cross the C
   ABI, which is UB).

## Key constraint

`catch_unwind` is a **no-op under `panic = "abort"`** — the process aborts before
the catch runs. So step 1 (drop `panic = "abort"` for the cdylib build) is a
prerequisite for step 2 (boundary `catch_unwind`); neither alone is sufficient.

## Implementation steps

### Build profile
- [ ] Stop building the cdylib with `panic = "abort"`. Options: remove `panic =
      "abort"` from `[profile.release]` (simplest; both binary + cdylib unwind,
      small binary-size cost from unwind tables), or introduce a dedicated
      cdylib profile / separate the crate so only the CLI keeps abort.
- [ ] Confirm `release-local` (plugin cdylib source) resolves to an unwinding
      build after the change.

### Panic barriers
- [ ] Wrap every `extern "C"` FFI entry point in `ffi.rs` with
      `std::panic::catch_unwind` returning a safe error/sentinel; log the panic to
      the ops log / plugin log. A panic must never unwind across the C ABI.
- [ ] Wrap the bodies of callbacks agent-doc hands to `lazily` (the code that runs
      on `agent-doc-crdt-*` threads) in `catch_unwind` so a panic there terminates
      only that unit of work, not the process.
- [ ] Audit for other spawned threads whose panic would currently abort.

### Regressions
- [ ] Test: an FFI entry point whose inner logic panics returns the error
      sentinel and does **not** abort the test process (guard with a helper that
      asserts the catch path; a spawned-thread harness for the callback case).
- [ ] Keep the existing `#utf8p` parser regressions green.

### Verification
- [ ] `make check` + full `cargo test`.
- [ ] `make install-full`, then live-editor proof: force a controlled panic path
      behind a debug flag and confirm the editor survives (operator-verify).

## Files likely to change

- `src/agent-doc/Cargo.toml` (profile)
- `src/agent-doc/src/ffi.rs` (FFI catch_unwind barriers)
- The agent-doc→`lazily` callback registration sites (CRDT thread bodies)
- `README.md` / `SPEC.md` if the FFI panic-safety contract is documented

## Exit criteria

- The cdylib cannot abort the host editor on a Rust panic; panics become logged
  errors/no-ops at the FFI and CRDT-callback boundaries.
- A deliberate panic behind a test/debug hook is proven not to crash the editor.

## Notes

- This needs its **own** clean cycle: it changes the build profile and the FFI
  hot path (mind the FlowCore token-budget gate on hot-path files), and it only
  takes effect after a rebuild + `make install` + supervisor recycle. Do not land
  it mid-session against a live supervisor / concurrent agent-doc session.
