# Plan: session_check pending-done ENOENT flake

## Goal

Stabilize the nondeterministic full-suite failure in `session_check::tests::session_check_skips_pending_done_warning_when_id_was_recorded`, which passed in isolation but failed once under `cargo test` with `No such file or directory (os error 2)`.

## Repro

1. Run `cargo test --manifest-path src/agent-doc/Cargo.toml`.
2. Watch for intermittent failure in `session_check::tests::session_check_skips_pending_done_warning_when_id_was_recorded`.
3. Confirm the same test still passes in isolation with:
   `cargo test --manifest-path src/agent-doc/Cargo.toml --bin agent-doc 'session_check::tests::session_check_skips_pending_done_warning_when_id_was_recorded' -- --exact`

## Investigation

1. Inspect `src/session_check.rs` around the failing setup path to find which file open/write/read can race or observe a missing temp path.
2. Check whether any shared temp-path, cwd, env, or git-root helper is reused across tests without serialization.
3. Re-run the surrounding `session_check` group under repeated / shuffled execution to isolate the conflicting test.

## Exit Criteria

- The full `cargo test --manifest-path src/agent-doc/Cargo.toml` suite passes without the ENOENT flake.
- The fix has a regression that fails before the change and passes after it.
