---
name: reference_rtk_diff_filter_reports_identical_for_differing_files
description: "RTK-wrapped per-file `diff` can report \"[ok] Files are identical\" for files that demonstrably differ — verify with grep/python instead."
metadata: 
  node_type: memory
  type: reference
  originSessionId: 1a36b238-2da1-45fc-aaf7-866edb8fd0fa
  modified: 2026-07-30T07:29:31.487Z
---

**`grep -rl` / `git grep -l` also return FALSE NEGATIVES (2026-08-02).** While auditing a
leaked local-dev credential in `src/equityfundingsource-dev`, `grep -rlF -- "$value" .` and
`git grep -lF -- "$value"` both printed **nothing** for a value that was sitting in
`.dev.vars` in the same directory — a security scan silently reporting "no exposure". A
`python3` `os.walk` + `bytes in data` scan of the same tree found two files. So grep is
**not** a safe fallback for the `diff` false-pass below: for any exhaustive content search
whose *absence* you intend to act on, use `python3` (or `rtk proxy grep` / `/usr/bin/grep`)
and confirm the scan finds a known-present control hit before trusting an empty result.

Observed again 2026-07-30 in `src/lazily-zig`, on `git diff --stat` this time:
the filtered wrapper reported `src/lazily/queue_family_conformance.zig | 2 +-` for a file
whose real unstaged diff was **2062 lines**. Reading the tree from that number produced a
completely wrong picture of what work was in flight, and cost a long detour chasing a
5-line source-line discrepancy in a Zig test failure. `rtk proxy git diff --stat` and
`/usr/bin/git diff --stat` both showed the true 2062.

**Applies to `git diff --stat`, not just per-file `diff`.** Before concluding anything about
the size or shape of a working tree, re-run through `rtk proxy` or `/usr/bin/git`.

Observed 2026-07-25 while re-syncing vendored conformance fixtures in `src/lazily-dart`:
`diff -rq` correctly flagged drifted files, but a per-file `diff <a> <b>` through the
RTK-filtered wrapper printed `[ok] Files are identical` for files whose contents were
provably different (`"model": "CellMap"` vs `"SourceMap"`).

**Why it matters:** this is a verification tool that returns a FALSE PASS. Anything that
concludes "already in sync" from it is unproven. Same failure class as
[[reference_cargo_check_intercepted_returns_stale_digest]] (bare `cargo check` intercepted →
stale "0 crates compiled") and [[feedback_rtk_masks_make_check_exit]].

**How to apply:** for content equality, use `python3` (grep is not trustworthy either — see
the 2026-08-02 note above), e.g.
`python3 -c "import json;print(json.load(open(a))==json.load(open(b)))"` for JSON, or
`cmp -s`/`sha256sum`. Reserve `diff -rq` for *discovering* which files differ, then confirm
the actual content with one of the above before acting on it.
