---
name: feedback_jb_sigabrt_stale_cdylib_panic_abort_coredump_diagnose
description: "JetBrains IDE crash during an agent-doc session = stale cdylib (panic=abort) hitting an already-fixed Rust panic; diagnose via coredumpctl, root is preflight stale_install"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 55c7b9cd-2652-4c3e-8327-600b979efec4
---

A JetBrains IDEA crash mid agent-doc session is almost always a **Rust panic inside the loaded cdylib**, amplified to a whole-process kill because the cdylib is built `panic = "abort"` (`src/agent-doc/Cargo.toml` `[profile.release]`, inherited by `release-local` which the plugin loads) with **no `catch_unwind`** at the FFI / `lazily`-CRDT-callback boundaries. Any panic on any thread → `abort()` → JVM `SIGABRT`.

**Diagnose:** `coredumpctl list | grep idea` → `coredumpctl info <pid>` (thread name like `agent-doc-crdt-*`, stack all inside `libagent_doc.so`) → `coredumpctl dump <pid> | strings | grep -A2 "panicked at"` gives the exact `file.rs:line:col` + message. The per-pid `/tmp/agent-doc-native-<pid>/libagent_doc-*.so` timestamp is the plugin's copy time, not the build. JVM writes **no** `hs_err`/`java_error_in` for a native cdylib abort. Note: the crash-moment `idea.log` is often overwritten by the restarted instance; `idea.1.log` only holds up to the last rotation.

**Root cause is usually staleness, not a live code bug:** the live JVM had loaded a cdylib built *before* the fix landed. Preflight's `stale_install` warning is the upstream signal. Example (2026-07-02): pid 2674606 crashed at `src/component.rs:443` "byte index N is not a char boundary; inside '❯'" — a UTF-8 byte-slice panic already fixed on 2026-06-28 by `bounded_preview` (commit `95ea1b7a`, now in `agent-doc-element/src/element.rs`). The stale install predated the fix.

**How to apply:** confirm the panic is already fixed in HEAD (git log -S the panic site), then the recovery is just a fresh `make install` (which the live system may have already auto-done). Do **not** force-disk the session doc — the crash is unrelated to session-doc authority. The durable amplifier fix (drop `panic=abort` for the cdylib + `catch_unwind` at FFI/thread boundaries) needs its own focused cycle: [[project_agent_doc_module_decomposition]] plan `tasks/agent-doc/plan-cdylib-panic-isolation.md` (`#cdylibpanic`). Same session's finalize wedge (`live_prompt_drift_after_preflight`) came from the *mid-session* install creating version skew — see [[feedback_live_prompt_drift_wedge_is_midcycle_recycle_not_staleness]].
