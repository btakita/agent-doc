---
agent_doc_session: 3d9ff8e4-bafd-4e1f-b48d-550d3c976904
agent: opencode
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---



# lazily-rs: Reactive Signals Library

<!-- agent:status -->

- [x] Phase 1: Research & design
- [x] Phase 2: Core signal primitives
- [x] Phase 3: Derived/computed signals
- [x] Phase 4: Effect system
- [x] Phase 5: Testing & benchmarks
- [x] Phase 6: Documentation & publish
<!-- /agent:status -->

<!-- agent:exchange -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260609-233146.md`*

Compacted content:
- Archived 7 response topic(s): Release + publish; Next Steps; #pz3k; 4 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57...

### Re: GitHub org naming — opus

> **Queue prompt:**
>
> do [#ms7x]


> **Queue prompt:**
>
> do [#kavj]


> **Queue prompt:**
>
> lazily-reactive is a bit long and difficult to remember. lazily-org is better. Is there an ever better name? lazily-root? lazily-super? lazily-hub? 


Best candidates, ranked:

1. **`lazily`** — shortest, most memorable, directly matches the project/crate identity. If available, this is the clear winner.
2. **`lazily-rs`** — follows Rust ecosystem convention (`tokio-rs`, `serde-rs`, `tauri-apps`). Natural fallback if `lazily` is taken.
3. **`lazily-hub`** — decent but adds an unnecessary qualifier; GitHub already shows it's an org.
- **Build:** `cargo build --features ipc-binary` passes; all 105 tests pass
- Wire each binding's CI to run conformance against `lazily-spec/conformance/`
- Initialize `lazily-zig` and add conformance test runner
- Add IPC + conformance test runner to `lazily-py`
- `lazily-zig` — submodule not initialized; conformance tests require IPC support first (backlog)
- `lazily-py` — no IPC module yet; conformance tests require IPC support first (backlog)
- All 105 tests pass, including 9 conformance tests
- Updated `tests/conformance.rs` to prefer `../lazily-spec/conformance/` when available, falling back to local `tests/conformance/`
- Added fixture format documentation to README.md with table of fixtures and instructions for new bindings
- Added `conformance/` directory with 6 canonical JSON fixtures (snapshot_minimal, snapshot_multi_node, snapshot_shared_blob, delta_sequential, delta_non_sequential, delta_shared_blob)
- **Spec:** Documented in `SPEC.md` under "Binary serialization (`ipc-binary` feature)"
- **Tests:** 7 binary-specific tests across `tests/ipc.rs`, `tests/conformance.rs`, `tests/ffi.rs` — round-trip, garbage rejection, size comparison
- **FFI:** `lazily_ffi_channel_send_binary`, `recv_binary`, `validate_binary`, `kind_binary`, `clone_binary` (`src/ffi.rs`)
- **Error types:** `EncodeError::Binary` / `DecodeError::Binary` variants
- **API:** `IpcMessage::encode_binary()` / `decode_binary()` via postcard (`src/ipc.rs:646-654`)
- **Feature flag:** `ipc-binary = ["ipc", "dep:postcard"]` in `Cargo.toml`

The others are weaker: `lazily-org` is redundant (GitHub already denotes orgs), `lazily-root` and `lazily-super` don't convey clear meaning.

**Recommendation:** try `lazily` first, fall back to `lazily-rs`. Updated review item #w30b to reflect this.

### Re: #kavj ipc-binary — opus

> **Queue prompt:**
>
> do [#kavj]


Already implemented. The `ipc-binary` feature is complete:

- **Feature flag:** `ipc-binary = ["ipc", "dep:postcard"]` in `Cargo.toml`
- **API:** `IpcMessage::encode_binary()` / `decode_binary()` via postcard (`src/ipc.rs:646-654`)
- **Error types:** `EncodeError::Binary` / `DecodeError::Binary` variants
- **FFI:** `lazily_ffi_channel_send_binary`, `recv_binary`, `validate_binary`, `kind_binary`, `clone_binary` (`src/ffi.rs`)
- **Tests:** 7 binary-specific tests across `tests/ipc.rs`, `tests/conformance.rs`, `tests/ffi.rs` — round-trip, garbage rejection, size comparison
- **Spec:** Documented in `SPEC.md` under "Binary serialization (`ipc-binary` feature)"
- **Build:** `cargo build --features ipc-binary` passes; all 105 tests pass

No work needed — marking `#kavj` as done.

### Re: #ms7x cross-lang conformance — opus

> **Queue prompt:**
>
> do [#ms7x]


Implemented the shared conformance test suite infrastructure:

**lazily-spec (canonical source):**
- Added `conformance/` directory with 6 canonical JSON fixtures (snapshot_minimal, snapshot_multi_node, snapshot_shared_blob, delta_sequential, delta_non_sequential, delta_shared_blob)
- Added fixture format documentation to README.md with table of fixtures and instructions for new bindings

**lazily-rs (reference implementation):**
- Updated `tests/conformance.rs` to prefer `../lazily-spec/conformance/` when available, falling back to local `tests/conformance/`
- All 105 tests pass, including 9 conformance tests

**Cross-binding status:**
- `lazily-py` — no IPC module yet; conformance tests require IPC support first (backlog)
- `lazily-zig` — submodule not initialized; conformance tests require IPC support first (backlog)

**Remaining work for full CI coverage** (added to backlog):
- Add IPC + conformance test runner to `lazily-py`
- Initialize `lazily-zig` and add conformance test runner
- Wire each binding's CI to run conformance against `lazily-spec/conformance/`

### Re: org naming — lazily taken — opus

> **Queue prompt:**
>
> lazily is not available. lazily is more than just rust. It's first implementation was python. lazily-hub seems the best so far. Is there a better option?


Since `lazily` is taken and the project is multi-language (Python-first), the name should be language-agnostic. `lazily-hub` is strong. Other viable options to consider:

- **`lazily-hub`** — clear, conveys ecosystem/hub purpose. Best of the remaining options.
- **`lazily-dev`** — common GitHub pattern for dev-tooling orgs (concise, 10 chars).
- **`lazily-signal`** — ties to the reactive signals concept, but longer.

If `lazily-hub` is available, go with it. `lazily-dev` is the only shorter alternative worth considering — everything else either adds noise or is too niche.
<!-- agent:boundary:c5d06868 -->
- **`lazily-signal`** — ties to the reactive signals concept, but longer.
- **`lazily-dev`** — common GitHub pattern for dev-tooling orgs (concise, 10 chars).
- **`lazily-hub`** — clear, conveys ecosystem/hub purpose. Best of the remaining options.
<!-- agent:boundary:ddd4177b -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" go=auto -->
- Create a lazily-hub org
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#jpf1] #webrtc-data=WebRTC data channel transport for peer-to-peer IPC (beyond signaling)
- [/] [#w30b] #gh-org=Create GitHub org (lazily-reactive or lazily) and transfer repos
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily-rs.done.md -->

<!-- completed work archived in tasks/software/lazily-rs.done.md -->
<!-- /agent:done -->
