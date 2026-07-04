---
name: feedback-rtk-masks-make-check-exit
description: "The rtk wrapper can report exit 0 for `make check` even when cargo tests fail — never trust the wrapped exit code; capture it explicitly and grep for FAILED"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 161a7f1c-ad91-4338-9595-917553467532
---

When running `make check` (or `cargo test`) in agent-loop / `src/agent-doc`, the RTK hook rewrites the command through its digest-runner, which **captures output to a log but can return exit code 0 even when the underlying cargo test suite FAILED**. A "Background command completed (exit code 0)" notification is therefore NOT proof the suite passed.

**Why:** the digest-runner's own success (it produced a digest) is what surfaces as the tool exit code, decoupled from the wrapped command's real exit. Observed 2026-05-30: a broken `run_integration` test (`codex_bare_run_inside_owning_pane_fails_before_nested_dispatch`) failed under `make check`, yet the wrapper reported exit 0 — and a commit (`6f91beb2`) was pushed with the failing test as a result.

**How to apply:** to verify a Rust change, do BOTH:
1. Run `(make check; echo "MAKE_CHECK_EXIT=$?")` so `$?` captures make's real exit *before* rtk sees it, and assert `MAKE_CHECK_EXIT=0`.
2. Grep the output for real failures: `grep -E "result: FAILED|[1-9][0-9]* failed|error\[E"` (ignore `0 failed` / test-name substrings like `..._fails_...`).

Do not declare "make check green" on the wrapper's reported exit code alone. `cargo-nextest` is not installed locally, so `make test` runs plain `cargo test --workspace --all-targets`. Relates to [[feedback-precommit-testing]] and [[feedback-cargo-build-release-after-route-edit]].

**RTK also masks clippy, AND local toolchain drifts behind CI (observed 2026-06-06, tsift #trt1p2 cycle).** `cargo clippy --workspace --all-targets -- -D warnings` run through the RTK hook prints a summarized `cargo clippy: No issues found` even when clippy actually fails — the real lint errors are swallowed. Compounding it: tsift CI's runner uses **rolling stable** (rolled to rustc 1.96.0), while the local toolchain was pinned older (1.94.0); `clippy::unnecessary_sort_by` got stricter in 1.96 and flagged three pre-existing `sort_by(|a,b| b.X.cmp(&a.X))` Copy-key reverse sorts in `packages/tsift-cli/src/commands/infra.rs` that 1.94 accepted — so local `make check` was green but CI's `clippy + test` job went red. To reproduce CI clippy locally: (1) `rustup update stable` to match CI's version (check the rust-X.Y.Z URL in the CI clippy error to see which version CI used); (2) run real clippy bypassing the RTK masking with `rtk proxy cargo +stable clippy --workspace --all-targets -- -D warnings`; (3) if it still finishes in ~0.1s it used cache — `cargo clean -p <crate>` (or touch sources) to force a real re-lint. Note `unnecessary_sort_by` only fires on Copy keys (suggest `sort_by_key(|b| std::cmp::Reverse(b.X))`); String-key single sorts and `.then(...)` compound sorts and `partial_cmp` float sorts do NOT fire.
