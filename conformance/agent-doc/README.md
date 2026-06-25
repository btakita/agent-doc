# agent-doc state cross-editor conformance fixtures (`#lazilystatesync5` / `#6n5j`)

These two files are **byte-identical vendored copies** of the canonical
lazily-spec conformance fixtures:

- `src/lazily-spec/conformance/agent-doc/snapshot_agent_doc_state.json`
- `src/lazily-spec/conformance/agent-doc/delta_agent_doc_state.json`

They are the single shared canonical input that pins the Rust authoritative
state graph and the JetBrains (Kotlin) + VS Code (JS) mirror graphs to one
expectation. A `snapshot` (epoch 3) followed by its `delta` (base_epoch 3 →
epoch 6) advances:

- `agent_doc.closeout.cycle` `preflight_started` → `committed`
- `agent_doc.queue.head` `selected` → `completed`
- adds an `agent_doc.transport.patch` node (phase `acked`)

The fixtures use the lazily-spec **generic graph** wire shape
(`node` / `state.Payload` byte arrays / adjacently-tagged `{ "CellSet": … }`
ops). The agent-doc FFI emits the **agent-doc** flattened shape
(`slot_id` / `type_tag` / base64 `payload` / `{ "op": "cell_set" }` ops —
`agent-doc-orchestration/src/state_wire.rs`). Each language's parity test reads
this canonical fixture, adapts the generic ops into its own mirror's apply
format, and asserts the derived projection summary converges to the same
canonical expectation declared in the fixture `assertions` block:

| field | snapshot | after delta |
|---|---|---|
| `cycle_phase` | `preflight_started` | `committed` |
| `queue_head_phase` | `selected` | `completed` |
| `epoch` | 3 | 6 |
| transport patch phase | (absent) | `acked` |

Pinned by:
- Rust: `agent-doc-orchestration/src/state_wire.rs` (`mod conformance_parity`)
- Kotlin: `editors/jetbrains/.../StateGraphMirrorConformanceTest.kt`
- JS: `editors/vscode/src/stateMirrorConformance.test.ts`

**Do not edit these copies independently** — if the lazily-spec source changes,
re-vendor both files and re-run all three suites so the languages stay in
lockstep.
