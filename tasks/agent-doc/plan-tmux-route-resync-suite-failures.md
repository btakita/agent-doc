# Plan: tmux route/resync suite failures

## Goal

Stabilize the current `cargo test` route/resync/sync tmux integration failures so `agent-doc` doc-surface changes can still clear the full verification suite.

## Evidence from May 3, 2026

- `cargo test` finished with `1763 passed; 24 failed`.
- Failure clusters:
  - `resync::tests::*wrong_window*` and retained-dead/stash purge coverage
  - `route::tests::*fresh_start*`, `*fresh_restart*`, `*tracked_codex_clear*`, and busy-reroute handoff coverage
  - `sync::tests::register_synced_files_*` and `tmux_router_sync_keeps_cross_root_columns_stable_when_focus_moves`
- Representative failure modes:
  - duplicate tmux session creation (`tmux new-session failed: duplicate session: test`)
  - mock prompt/readiness assertions failing (`mock agent should be ready`)
  - fallback session / auto-start invariants failing in route recovery

## Next steps

1. Reproduce the 24 failures in an isolated tmux environment without concurrent cargo jobs.
2. Separate duplicate-session contamination from true prompt/readiness regressions.
3. Fix the route/resync/sync helpers or test harness setup so the full suite is green again.
4. Rerun `cargo test`, `cargo run --quiet -- audit-docs`, `cargo build --release`, and `cargo install --path . --force`.
