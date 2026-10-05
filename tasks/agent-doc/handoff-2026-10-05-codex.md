# Handoff to Codex: agent-doc-bugs batch (2026-10-05)

The Claude Code session that coordinated this batch is at its weekly token limit. All
in-flight work lives in git worktrees and is committed (WIP commits included). Nothing
is pushed, installed or released yet. Session document:
`tasks/agent-doc/agent-doc-bugs.md`.

## Step 0: take ownership of the queue claims

Claude claimed the queue heads for its subagents, and claimed heads are skipped by
preflight. Before resuming:

1. Make sure no Claude subagent is still writing to a worktree:
   `pgrep -x claude` and `git -C <wt> log -1 --format=%cr`. If a worktree's last
   commit is recent and Claude is still running, wait or leave that item alone.
2. List the claims with `agent-doc queue claims tasks/agent-doc/agent-doc-bugs.md`.
   Release each claim you take over with
   `agent-doc queue release tasks/agent-doc/agent-doc-bugs.md --item '<#id or line text>'`.
   Without a release they expire by TTL anyway (subagent claims about 2h, coordinator
   claims about 3–4h after 00:15–00:32 EDT).

## Step 1: resume each worktree

Each worktree has a `HANDOFF.md` at its root (goal, done, remaining, commands). Read it
first, then finish the remaining steps. Worktrees are siblings under
`/home/brian/work/btakita/agent-doc-wt/`, outside the IDE-watched project. Never run two
agents in one worktree, and never edit `src/agent-doc` (the main checkout) directly.
Another session has uncommitted edits there.

| Queue item | Worktree / branch | State at handoff | Depends on |
|---|---|---|---|
| `do [#jbstagefallback]` | merged into `batch-0505` (from `jbstagefallback` @ 58ce2d911) | DONE, JB suite green | — |
| alt+shift agent-doc menu | merged into `batch-0505` (from `altshiftmenu` @ ce8c1707b) | DONE. Restores Alt+Space on the XWin keymap; the operator should confirm "alt+shift" meant Alt+Space | — |
| `#gh-fix` GH #136 (stale stash supervisor pane admitted as a layout column) | `gh136` @ 9bb7edb2b | DONE. `make check` green (nextest 11027 passed, clippy clean, TLA incl. the new StaleColumnRecycle.tla with 5 wedges). Root cause: an install_fanout request expiring past its TTL hid the stale-supervisor request, plus unbounded stash promotion. Fix: stale-binary requests never expire, a pending/overdue state, a pure `stale_focus_admission` (no widening, no overdue promotion), and `gated_documents` in the layout receipt. Remaining follow-ups (from the subagent; add as backlog): (a) enforce columns <= observed_panes for ALL publishers (editor_surface); (b) investigate the 60s focus settle and the `superseded` gen-reset race on 1061.md; (c) supervisors already stale on old code still need a one-time recycle; (d) republish the layout when a recycle completes; (e) bound stale panes in non-`stash` windows. The subagent's empty `git stash pop` touched another worktree's autostash (`stash@{0}`); it reverted the two files, and the stash is intact | — |
| `do [#netadv1]` channel-assumption audit | `netadv1` @ fd64518b3 | DONE (docs only). See docs/reference/network-channel-audit.md: 12 ranked risks R1-R12 and 23 one-shot candidate fixes F1-F23, plus a README table. Top risks: R1, a 1s focus timeout SIGKILLs a busy controller; R2, a slow attach is treated as a definitive refusal and drops the editor from the cut; R3, a slow connect deletes a live socket file. netadv5 is unblocked. Convert F1-F23 into backlog items. Note: every channel is workspace-local and Zscaler sits only between the thin client and the backend, so the danger is a short timeout being read as a verdict | — |
| `do [#netadv2]` NetChannel.tla | `netadv2` @ 6fdaa8607 | DONE. `make tla` green, 25 modules; 4 wedges and the Reach config fail on their intended properties. API is in formal/tla/README.md. Delivery fairness is strong (SF) per message. netadv3 is now unblocked on its netadv2 side | — |
| `do [#netadv4]` SimWorld network layer | `netadv4` | IN PROGRESS | — |
| `do [#netadv7]` cargo-fuzz + proptest | `netadv7` | IN PROGRESS | — |
| `do [#netadv3]` re-check hot-path models over NetChannel + fix one-shot defects F1-F23 | `netadv3` (from batch-0505) | IN PROGRESS; see HANDOFF.md | — |
| `do [#netadv5]` timeout-as-verdict fixes R1-R3,R5-R9 + RTT budget tests | `netadv5` (from batch-0505) | IN PROGRESS; see HANDOFF.md | — |
| `do [#netadv6]` deterministic simulation fuzzing | not started | HELD | netadv4 |
| `release + publish` | — | HELD | integration below |

Plan for all netadv items: `tasks/agent-doc/plan-network-adversarial-correctness.md`.

## Step 2: integrate serially, one install per batch

1. `batch-0505` already holds jbstagefallback, altshiftmenu, gh136, netadv2 and netadv1
   (merged 2026-10-05; TLA README and run_tla.sh conflicts union-resolved). A `make
   check` on it was started; re-run it if the result isn't recorded here. Merge the
   remaining netadv branches one at a time. Delete each branch's `HANDOFF.md` in the merge.
2. JetBrains version conflicts in `editors/jetbrains/gradle.properties`: take one side,
   then `python3 scripts/check_plugin_versions.py --bump JetBrains`, `git add` the file,
   and re-run `python3 scripts/check_plugin_versions.py` (it must exit 0).
3. Run `make check` in the batch worktree and capture `$?` explicitly (RTK can mask
   it). Do not waive red tests.
4. Fast-forward or merge `batch-0505` into `src/agent-doc` `main`. Leave the other
   session's uncommitted files there untouched; commit only your own paths.
5. Run `make install` once, from `src/agent-doc` (per-fix `make install` only, never
   `cargo build --release` mid-turn).
6. Close the items in the session document through agent-doc (`--done jbstagefallback`,
   `--done netadvN`, and quote and strike the free-text heads). Close GH #136 after the
   release (`#gh-fix` = fix, then close on release).

## Step 3: release + publish

Follow the repo release flow (VERSIONS.md and pyproject first, then a tag push, which
auto-publishes the GitHub Release and PyPI). Add `editors/release-parity.tsv` rows for
the JetBrains changes. Run `cargo build --release` inside `src/agent-doc` after
publishing so `.bin/agent-doc` isn't stale. If gh136 or the netadv branches aren't
ready, release what is integrated and leave the rest queued.

## Operator requirements captured this session

- agent-doc must be **provably correct under varying network conditions**: Coder
  remote workspace plus Zscaler, meaning high and variable latency, half-open stalls
  and message loss. Safety must hold under delay, reorder, drop, duplicate and
  reconnect, with no timeout in a safety argument. Liveness only under fairness plus
  retransmission.
- Lower latency by removing serial round trips, not by shrinking timeouts that absorb
  network delay.
- Simulation must cover network conditions; fuzzing should find new issues
  proactively (netadv6, netadv7).
