---
name: reference_install_full_editor_plugin_thrashes_running_ide_replica
description: "make install-full drops a new JB plugin into RUNNING IDEs and induces a replica register/deregister storm that wedges Run Agent Doc until the IDE restarts"
metadata:
  node_type: memory
  type: reference
---

`make install-full` (agent-doc `AGENTS.md` Release Process step 4) ends with
`install-editor-plugins`, which copies the freshly built JetBrains package into
every **already-running** IDE installation and prints "Restart running IDEs to
activate the package generation." `lib-install` swaps the native cdylib at the
same time. The running IDE keeps executing the OLD Kotlin plugin generation
against the NEW native library.

**Symptom (observed 2026-08-03, release 0.35.119):** the session document's own
replica enters a register/deregister ping-pong with the controller. Measured in
`src/haiven-dev/.agent-doc/logs/ops.log`, `crdt_replica_deregister` for
`haiven-dev.md` per hour:

```
03:00Z     14      <- normal
04:00Z   1052      <- install-full finished 03:44Z
```

~19 cycles/min, one every ~3s, indefinitely. Sibling non-session markdown files
log `controller_crdt_replica_refused ... reason=detached_authority` on the same
cadence.

**Consequence:** the document never reaches a settled replica, so JB **Run Agent
Doc does nothing** — and it fails *before* the route layer, so `dispatch_attempts`
in that repo's `state.db` gets **no new row at all**. Do not read "no dispatch
row" as "the operator did not click"; check the register/deregister rate first.
A cycle left in `preflight_started` with no `committed` partner is the other tell.

**Fix: restart the JetBrains IDE.** `agent-doc admin reload-lib` does NOT help —
it only swaps the cdylib, not the Kotlin plugin (same reason
[[reference_agent_doc_stale_plugin_ack_wedge_reload_lib_unwedge]] needs a
restart). `admin recycle` does not help either; the mismatch is in the IDE
process.

**How to apply:** `install-full` is a release step, but its editor-plugin stage
is hostile to live IDE sessions. When other sessions are live, install the
binary + skills + cdylib manually and skip `install-editor-plugins`:

```sh
cargo build --profile release-local --target-dir target/local-install --bin agent-doc
./target/local-install/release-local/agent-doc binary-install --source target/local-install/release-local/agent-doc
./target/local-install/release-local/agent-doc skill install --all
cargo build --profile release-local --target-dir target/local-install --lib
CARGO_TARGET_DIR=target/local-install agent-doc lib-install --profile release-local
```

Then tell the operator to restart the IDE at their convenience. Related:
[[feedback_always_install_and_recycle_agent_doc]],
[[feedback_jb_plugin_cdylib_hot_reload]].

**Second trap from the same run:** `install-full`'s final
`cleanup-build-artifacts` deletes `target/`, which leaves
`.bin/agent-doc -> src/agent-doc/target/release/agent-doc` dangling. Re-run
`cargo build --release --bin agent-doc` afterwards
([[reference_bin_agent_doc_symlink_is_reporoot_relative]]).
