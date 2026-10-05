# HANDOFF: netadv7 (coverage-guided fuzzing of untrusted input)

Item: #netadv7, plan /home/brian/work/btakita/agent-loop/tasks/agent-doc/plan-network-adversarial-correctness.md
Worktree: /home/brian/work/btakita/agent-doc-wt/netadv7, branch netadv7. Do NOT push/install/release/merge.

## Goal
cargo-fuzz targets (excluded from workspace) for IPC wire decode, markdown patch/component
parsers, frontmatter, CRDT op decode; corpus seeded from redacted real traffic; stable
corpus-replay tests in `make check`; proptest model-based test for a TLA+ transition
function; every crasher -> regression test + fix.

## Done
1. `agent-doc-fuzz-harness` workspace crate: harness fns ipc_wire, markdown_patch, frontmatter,
   crdt_update, crdt_edits (src/lib.rs); tests/corpus_replay.rs replays fuzz/corpus + fuzz/regressions
   on stable (runs in `make check` via nextest --workspace); examples/seed_corpus.rs regenerates corpus.
2. `fuzz/` cargo-fuzz crate excluded from the root workspace (`exclude = ["editors/zed", "fuzz"]`),
   5 targets; fuzz/seed/redact.py redacts real traffic; corpus ~285KB from redacted
   .agent-doc/logs/ipcfullprompt docs, responses, legacy yrs, plus builder-generated IPC lines.
3. `make fuzz` (opt-in; nightly + cargo-fuzz; FUZZ_SECONDS default 60). Not part of `check`.
4. Model-based proptests: agent-doc-supervisor/tests/generation_transition_model.rs
   (SupervisorGenerationTransition.tla <-> generation_transition_admission: shared table,
   TLA text pin, exhaustive BFS, proptest traces + fairness) and
   agent-doc-ipc-protocol/tests/ipc_build_identity_model.rs (IpcBuildIdentity.tla <-> real handshake).
   Mutation-checked: gate mutations are killed.
5. Crashers found and fixed (each has a unit regression test that fails before the fix):
   a. template repair_duplicate_exchange_opener panicked on a NESTED exchange (reversed slice);
      now bails. The realtime-io integrity predicate treats Err as not-single.
   b. frontmatter write_preserving used raw_frontmatter_yaml (empty `---\n---` block mis-split), so the
      body was duplicated into the frontmatter. Now uses split_frontmatter.
   c. frontmatter write_preserving with a bare `\r` in a value produced a duplicate key and an unparsable
      doc. Now falls back to write and self-verifies the splice.
   d. frontmatter write dropped a block scalar's trailing newline when it became the last key.
      write now adds a blank line before the fence only when needed.
   e. frontmatter write dropped legacy queue_active beside an unparseable `queue:` value, so the
      queue state changed. Now kept.
   f. merge MultiNodeState::decode trusted the u32 node count as Vec capacity (9-byte input, 100GB malloc).
   g. merge decode_columnar_ops overflowed i64 on hostile counter deltas (panic under overflow checks).
6. Fuzz runs: several rounds of 90-120s per target. All 5 targets were clean on the final round.

## Remaining
None for #netadv7. Not pushed/installed/released/merged (per instructions).

## Commands / status
- cargo-fuzz (scratch install): PATH=<scratchpad>/cf/bin:$PATH; `make fuzz FUZZ_SECONDS=120`.
- `make check` (via `rtk proxy`, exit captured explicitly): MAKE_CHECK_EXIT=0; nextest 11042 passed, 265 skipped.
