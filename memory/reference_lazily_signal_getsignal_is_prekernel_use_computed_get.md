---
name: reference_lazily_signal_getsignal_is_prekernel_use_computed_get
description: "lazily `signal`/`get_signal`/`ThreadSafeSignalHandle` are the deprecated pre-kernel-v2 shape; derive with `computed` and read with `get`"
metadata: 
  node_type: memory
  type: reference
  originSessionId: 02651d81-aaf2-4f85-92ea-fb7f53827348
  modified: 2026-07-25T22:45:33.057Z
---

The current lazily cell kernel (`#lzcellkernel`) is **`Source` / `Computed` / `Effect`,
read with `ctx.get(&cell)`**. `ctx.signal(..)` / `ctx.get_signal(..)` /
`ThreadSafeSignalHandle` are the older two-node shape (a memoized slot plus a puller
`Effect`) and must not be used in new code. See [[project_lazily_memo_rename_plan_status]].

**They are not marked `#[deprecated]`, which is why they keep getting reached for.**
On single-threaded `Context` they already just delegate (`signal` = `computed().eager()`,
`get_signal` = `get`) so deprecating them is safe today. On `ThreadSafeContext`,
`signal` still builds the real two-node shape and there is no eager thread-safe
`Computed` yet — so deprecating that half is blocked on adding one (`make_eager`
exists only in `src/context.rs`, not `src/thread_safe.rs`).

Enforcement that exists now, in agent-doc:
`agent-doc-state-scope/tests/architecture_scope_guard.rs` fails `make check` on
`ctx.signal(` / `get_signal(` / `SignalHandle` in workspace sources unless the line
carries `// #lzcellkernel-allow: <reason>`. It deliberately matches `ctx.signal(`
rather than `.signal(` so the unrelated stop-flag API (`stop.signal()`) does not trip.

The single-source fix — `#[deprecated]` in lazily-rs, matching the existing
`get_cell`/`set_cell` deprecations — would cover all Rust consumers, not just
agent-doc. Not done: it emits warnings across ~10 repos at once and
[[feedback_concurrent_sessions_force_push_shared_superproject]] makes an unrequested
shared-submodule change risky.

**How to apply:** when writing a derived reactive value in any agent-doc/lazily Rust
code, reach for `ctx.computed(..)` + `ctx.get(&cell)`. If a compile error suggests
`get_signal`, that is the old API surfacing, not the fix.
