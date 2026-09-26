---
name: reference_concurrent_make_test_deletes_nextest_binary_midrun
description: "A concurrent session's `make test` in the shared agent-doc tree deletes the nextest test binary mid-run: `[double-spawn] failed to exec ... No such file or directory` is a collision, not a test failure."
metadata:
  node_type: memory
  type: reference
---

In the shared `src/agent-doc` working tree, a second session running `make test`
makes cargo GC stale `target/debug/deps/<suite>-<hash>` binaries while YOUR
nextest run is executing them. The result looks like a hard test failure:

```
FAIL [ 0.002s] agent-doc::test_cli <some_test>
  error: [double-spawn] failed to exec `.../target/debug/deps/test_cli-70ac6f34b1ec1096` ...
  Caused by: No such file or directory (os error 2)
  (test failed with exit code 70)
Cancelling due to test failure: 31 tests still running
Summary [113.632s] 257/9770 tests run: 256 passed, 1 failed, 259 skipped
```

Observed 2026-09-26: `make check` exited 2 this way with 20 `test_cli-*` binaries
in `deps/` and another session's `cargo nextest run --workspace` live. A re-run on
an idle tree was fully green (9770/9770 passed, 259 skipped).

**Tells it is a collision, not your code:** the failure is `failed to exec`, not an
assertion; it fires at ~0.002s; the named test is unrelated to your diff; and the
run cancels early with thousands of tests unrun.

**Why:** cargo's target dir is shared state, and `--all-targets` rebuilds rotate
the hashed binary names other runs already resolved.

**How to apply:** do NOT waive it as flaky and do NOT accept it as a red suite —
confirm the tree is idle with `pgrep -x cargo-nextest; pgrep -x cargo; pgrep -x rustc`
(match the BINARY name, see [[reference_pgrep_f_wait_loop_matches_its_own_command_line]]),
then re-run and assert the runner-native counts. See
[[reference_local_check_green_while_head_broken_shared_tree]] and
[[feedback_rtk_masks_make_check_exit]].
