# HANDOFF: netadv7 (coverage-guided fuzzing of untrusted input)

Item: #netadv7, plan /home/brian/work/btakita/agent-loop/tasks/agent-doc/plan-network-adversarial-correctness.md
Worktree: /home/brian/work/btakita/agent-doc-wt/netadv7, branch netadv7. Do NOT push/install/release/merge.

## Goal
cargo-fuzz targets (excluded from workspace) for IPC wire decode, markdown patch/component
parsers, frontmatter, CRDT op decode; corpus seeded from redacted real traffic; stable
corpus-replay tests in `make check`; proptest model-based test for a TLA+ transition
function; every crasher -> regression test + fix.

## Done
1. Survey. Toolchains: nightly present; cargo-fuzz 0.13.2 installed to scratch root
   /tmp/claude-1000/-home-brian-work-btakita-agent-loop/c6814369-5609-4ecd-bdf6-1f2a5cca6d15/scratchpad/cf/bin
   (add to PATH; not installed globally).
2. Entry points chosen:
   - IPC: agent_doc_ipc_protocol::{validate_ipc_hello, validate_ipc_hello_ack, classify_socket_receipt,
     message_is_reload_library, message_requests_early_receipt, CallbackRequest/Response},
     agent_doc_state_wire::WireDeltaOp (serde round-trip).
   - Markdown: agent_doc_template::parse_patches, agent_doc_element::element::parse,
     agent_doc_markdown_lossless::{parse, project/restore} (claims render(parse(doc)) == doc).
   - Frontmatter: agent_doc_frontmatter::{parse, write, write_preserving}.
   - CRDT: agent_doc_merge::crdt_sync::decode_update_ops (+ ReplicaState::apply_update),
     agent_doc_merge::crdt::MultiNodeState::decode.
   - Model-based: SupervisorGenerationTransition.tla <-> agent_doc_supervisor::lifecycle::generation_transition_admission.
3. Suspected crashers (to confirm by fuzzing): MultiNodeState::decode `Vec::with_capacity(count)` with
   attacker-controlled u32 count (alloc abort); decode_columnar_ops i64 `previous_counter += delta`
   and `counter - delta` overflow (debug panic).

## Remaining
1. Create workspace member crate `agent-doc-fuzz-harness` (pure harness fns + corpus replay tests).
2. Create `fuzz/` cargo-fuzz crate (own [workspace], excluded from root workspace) calling the harness.
3. Seed corpus from /home/brian/work/btakita/agent-loop/.agent-doc/ (redacted, small).
4. Run each fuzz target 60-120s on nightly; fix crashers + regression tests.
5. proptest model-based test for generation_transition_admission (and optionally IpcBuildIdentity).
6. `make check` with explicit exit status; commit; update this file.

## Commands / status
- `make check` not run yet.
