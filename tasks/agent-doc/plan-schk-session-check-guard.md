# Plan — `agent-doc-bugs` `#schk`

- [x] Inspect the current Codex-facing skill/runbook path and identify where end-of-turn `session-check` is missing after response persistence.
- [x] Update the minimal `src/agent-doc` instruction surface so the Codex path requires `agent-doc session-check <FILE>` after `finalize` or manual `write --commit`, and states that a failing check blocks success reporting.
- [x] Update specs/docs only where needed so the Codex post-write guard is explicit and aligned with the binary contract.
- [x] Add regression coverage for bundled/generated/installed Codex instruction content containing the new `session-check` guard.
- [x] Run the requested verification in `src/agent-doc`: targeted tests, `make check`, `cargo build --release`, `cargo install --path .`, and refresh installed local skill content.
- [x] Patch the result back into `tasks/agent-doc/agent-doc-bugs.md` and mark `#schk` complete there.
