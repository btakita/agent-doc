# Editor parity

`plugin-parity.tsv` owns capability coverage; `release-parity.tsv` records the
editor impact of each release. Every new Cargo version must have exactly one
release audit row before `make check` passes. Update the row after reviewing
shared-core changes and any editor-specific adapter changes; a row is a scope
record, not a substitute for tests.

`make check` includes `editor-parity`: JetBrains and VS Code unit suites, the
real native-peer/controller harness, required capability declarations, and the
current release audit. CI installs the matching Java/Node build prerequisites
and runs the same target. A failed peer or missing release row blocks the gate.

The current supported-peer contract covers operator-text authority, transport
receipts, controller reboot recovery, and IDE-hosted tmux (host-dependent in
JetBrains). Shared core changes in 0.35.340–0.35.344 apply to both peers. The
0.35.342 JetBrains precheck supplements the shared controller's stale-base
refusal; the controller protects both adapters. In 0.35.345 JetBrains needed a
stdout-only bootstrap fix; VS Code discovers the native library directly.

Known differences remain visible: lossless-tree capability is supported only in
JetBrains; typed editor intents are staged in both; native hot reload is
conditional on the host. Zed remains staged for the native-peer contract and is
not counted as proven parity. Changing these declarations requires adapter and
conformance coverage, not simply changing a status cell.

Headless native tests prove transport/FFI behavior. IDE UI, VFS wiring, and
platform-specific terminal behavior still need their documented host smokes.
