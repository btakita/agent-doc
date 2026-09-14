---
name: reference_parallel_subagents_share_one_scratchpad_dir
description: "Fan-out subagents inherit ONE scratchpad path, so fixed filenames like msg.txt collide mid-task between concurrent agents"
metadata: 
  node_type: memory
  type: reference
  originSessionId: 311c3110-b232-4f94-8665-f02e34d986e2
  modified: 2026-08-03T01:05:34.130Z
---

Subagents dispatched from the same session all inherit the **same** scratchpad
directory. A fixed filename is therefore shared state, not per-agent state: during
the nine-binding `#lzblobbackendstrict` fan-out, a lazily-cpp agent overwrote
`scratchpad/msg.txt` while a lazily-js agent was using it for a commit message. The
js commit body was correct only because git had already consumed the file — the
window is real and the failure would be a commit carrying another repo's message.

**How to apply:** when briefing a fan-out, tell each agent to use a filename keyed to
its repo (`msg-lazily-js.txt`) or to pipe the message via a heredoc into
`git commit -F -` rather than staging it through a file. Do not assume "session
scratchpad" means "per-agent scratchpad".

Related: [[feedback_never_dispatch_concurrent_agents_into_one_repo]] — that rule keeps
agents out of each other's repos; this one keeps them out of each other's temp files.

## 2026-09-12: this fired for real, and a picked-up stale copy INVERTS results

"Key filenames per repo" understates the hazard. Measured during a ten-repo
fan-out:

- **lazily-cs had a `lazily-zig` Makefile restored into it** from a
  generically-named `Makefile.orig` another repo's agent had overwritten.
  Caught by `git diff` before it reached a measurement.
- **kt's `$SP/Makefile.orig` held zig content overwritten at 15:28:16** -- its
  last restore from that filename was ~15:26. A two-minute miss.
- **dart's own subdirectory held 64 files from an EARLIER dart agent**,
  including a pre-fix copy of the same guard whose filename differed by a
  **word-order transposition** (`fixed-guard.sh` vs `guard.fixed.sh`) and which
  was **409 bytes** from one of dart's live working files. Had it been picked
  up, every "post-fix RED" would have been a **pre-fix run**.
- **cpp: the directory holds 11,220 files**, with ten bindings' copies of the
  same `check-ci-reach.sh` inside 2KB of each other, four written concurrently,
  the closest 241 bytes off.

**The failure mode is inverted results, not broken ones** -- a stale copy of the
right file produces measurements that look like proof and mean the opposite.

**The harness gap that makes it dangerous** (dart): rows that expect **GREEN**
-- clean-tree runs, positive controls -- pass **vacuously** under
contamination. Neither a mutation-applied assert nor a declared expected
direction notices a wrong input, and the healthy baseline is captured from the
same possibly-contaminated tree. "I restored from git" is the weaker claim.

**Working rule**: a repo-private subdirectory, repo-prefixed filenames, select
your own files by **mtime** (not by guessing names), and **verify every restore
input against `git show` before each case, refusing to measure otherwise** --
including the green rows. rs found two of its own harness bugs that way rather
than by luck.

**Also**: RTK's diff filter emits a blank line, so `git diff HEAD | wc -l`
reads **1** on an empty diff. Use `rtk proxy git diff`, or compare bytes
against the git object.
