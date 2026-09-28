---
name: reference_ipc_build_mismatch_was_a_clock_not_a_code_difference
description: "Before agent-doc 88b21fea6 the IPC build_id suffix was a build-script wall clock, so `IPC build mismatch` between two same-version peers usually meant NOTHING was different — and a real member-crate change handshook as identical; historical build-mismatch notes must be reread with that in mind"
metadata:
  node_type: memory
  type: reference
---

**Fixed in `88b21fea6` (2026-09-28, `#ipcverhandshake`).** Every
`IPC build mismatch: listener=0.35.X+<digits>, client=0.35.X+<digits>` observed
**before** that commit is untrustworthy evidence in both directions, so any older
note reasoning from one needs rereading.

The suffix was `SystemTime::now()` captured when the root build script ran. It is
now a 16-hex SHA-256 digest of the workspace's build-relevant sources
(`agent-doc-hash::source_digest`). Shape tells you which you are looking at:
all-digits = the old clock, hex = the fix.

**A same-version mismatch was usually FALSE.** This repo's Makefile guarantees
it: `make check` builds into `target/`, `make install` into
`target/local-install`. Two target directories = two build-script runs = two
clock readings for one tree. So a binary from `make check` and a binary from
`make install` could never handshake, no matter how identical. Also triggered by
`cargo clean`, a second checkout, and CI vs local.

The cost was total: `editor_attached_model_missing` survives its bounded
observation, then
`realtime_doc_resolve_missing_replica_terminal_rebuild_failed` +
`realtime_doc_resolve_disk_read_refused`, `operator_action=none`. Measured
2026-09-28T05:08Z on `tasks/agent-doc/agent-doc-bugs.md`: rejections at ~2/s
between `+1790563363` and `+1790567494`, every command against the document
refused. The last rejection in ops.log is 05:08:57Z; after the install there
have been none.

**A real difference was usually INVISIBLE.** `build.rs` declared
`rerun-if-changed=src/`, naming only the root package, and a cargo build script
is never re-run for a change in a *different* package — all ~140 workspace
members are different packages, the IPC crates included. Concretely:
`~/.cargo/bin/agent-doc` was written 23:51:45 advertising `+1790563363`
(22:42:43), from a tree whose `agent-doc-crdt-relay-io/src/lib.rs` changed at
23:25.

**Diagnostic consequence that bit this investigation.** Because the stamp lied,
"which code does this binary actually contain" could not be read off
`--version` + stamp at all; it had to be inferred from file mtimes against
`git log`. Post-fix the id answers it directly — two binaries with the same
`build_id` are the same tree, full stop.

Note the distinct, unaffected identity: `ControllerBinaryIdentity`
(`project_controller::current_binary_identity`) is the executable's path, len
and mtime, and SPEC.md's "newer same-version build timestamp" directional
comparison refers to *that*. The IPC `build_id` is compared for equality only
and is never ordered — do not confuse the two when reading a stale-controller
diagnostic.

Related: [[reference_cdylib_reload_strands_attached_document_replica]] is a
DIFFERENT class that produces the same terminal pair, and it still requires an
operator tab reopen; see [[reference_agent_doc_recovery_index]].
