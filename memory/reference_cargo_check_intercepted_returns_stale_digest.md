---
name: reference_cargo_check_intercepted_returns_stale_digest
description: "in agent-loop, a bare `cargo check` is intercepted and can report `0 crates compiled` / `Finished in 0.08s` on freshly edited files — touch the file or use tsift digest-runner to get a real compile result"
metadata: 
  node_type: memory
  type: reference
  originSessionId: c54d12a1-95e4-49a0-b66d-0178f0a3a3d8
  modified: 2026-07-25T18:55:56.242Z
---

In this workspace a bare `cargo check -p <crate>` is rewritten by the PreToolUse
hook and can come back as:

```
cargo build (0 crates compiled)
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.08s
```

**on a crate whose source you just edited** — no `Checking <crate>` line. `command
cargo` and `rtk proxy cargo` do not bypass it. Taking that at face value means
believing an edit compiled when nothing was rebuilt.

**Get a real result:**
```bash
tsift --envelope digest-runner --kind log --path . \
  --shell-command 'touch <edited file> && cargo check -p <crate> 2>&1 | tail -30'
```
then read the artifact log (`report.artifact.path`) — the digest's own summary
collapses to "command finished without log signals" and hides the compiler output.
The `touch` is what actually forces the rebuild; with it you get the expected
`Checking <crate> …` line.

Confirm the real work happened by looking for `Checking`/`Compiling` lines, not by
the absence of errors. `make check` is unaffected (it builds for real) and stays the
authoritative gate — but note it exits **2** on a clippy failure *before* any test
runs, so `grep -c 'result: FAILED'` returns 0 on that failure; check the recorded
exit code too. Related: [[feedback_rtk_masks_make_check_exit]].
