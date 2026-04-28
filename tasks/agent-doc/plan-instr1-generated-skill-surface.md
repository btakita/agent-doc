# Plan — `agent-doc-bugs` `#instr1`

- [x] Inspect the current `src/agent-doc` skill/install generation path and identify where Claude/Codex hot-path instructions still diverge procedurally.
- [x] Replace brittle harness-specific skill generation with a single shared generated instruction surface whose allowed differences are limited to harness invocation wording.
- [x] Update the product/spec docs so the shared-generation and completion-boundary contract is explicit.
- [x] Add regression coverage for generated-content parity, installed-file parity, and completion-boundary language.
- [x] Run the requested verification in `src/agent-doc`: targeted tests, `make check`, `cargo build --release`, and `cargo install --path .`.
- [x] Patch the result back into `tasks/agent-doc/agent-doc-bugs.md` and update the pending item state.
