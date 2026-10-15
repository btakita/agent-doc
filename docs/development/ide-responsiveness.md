# Keeping the IDE Responsive Under Build Load

agent-doc development runs many concurrent sessions, and each one may build,
test, and model-check in its own worktree. This page records what made IntelliJ
IDEA lag while the operator typed, what the repository now does about it, and
which settings only the operator can change (`#keepidesnappy`).

## What was measured (2026-10-15, 32-thread Ryzen 9 9950X3D, 186 GiB RAM)

| Source | Evidence |
|---|---|
| Concurrent worktree builds | Load average 115 (15 min), 427 peak during `make check`; ~10 `rustc`/`rust-lld`/`clippy-driver` processes at 60-270% CPU each, plus Gradle and TLC JVMs. |
| Priority inversion | `ananicy-cpp` (CachyOS rules) classes `tmux` as `Doc-View` (nice -4) and `idea` as `Heavy_CPU` (nice +9, best-effort I/O level 7). Every agent session and every build it starts inherits -4 from the tmux server, so builds outranked the editor by 13 nice levels. |
| IDEA itself | 130-280% CPU: six G1 concurrent-mark threads at ~20% each, the C2 JIT, and the EDT at ~28% (heap 2.6 GiB used of 16 GiB, so not heap exhaustion). |
| IDEA Git integration | In 3 of 5 `jstack` samples a worker was in `forkAndExec` for `git4idea` (`GitBranchIncomingOutgoingManager.getNumberOfCommitsBetween`, `GitStashTracker`). The project registers ~70 submodules as VCS roots, and every agent commit invalidates refs. |
| IDEA VFS refresh | The longest freeze (13 s, `threadDumps-freeze-20261015-161354`) was the EDT inside a transferred write action running `IndexedFilesListener.scheduleForIndexingRecursively` over a newly created directory tree: a large directory appeared inside a content root. |
| Memory | `/tmp` (tmpfs) held 110 GiB, nearly all of it pushed into zram swap (161 GiB of 186 GiB used, ~58 GiB compressed); `kswapd0` used ~57% CPU, and 1.3 GiB of IDEA's own pages were swapped out, so typing could fault on a compressed page. The largest entries were one copied checkout with its `target/` (34 GiB) and ~103,000 leaked `.tmp*` test-fixture directories. |
| agent-doc plugin | No agent-doc frame on the EDT in any freeze dump. Its per-keystroke listeners already record O(1) events and defer work to worker threads. Two EDT costs remained: the visual-highlight refresh (fixed, below) and the remote CRDT apply (`[crdt-perf] remote-apply-edt` 86-222 ms on a 32 KB document, once per remote delivery, not per keystroke). |

## What the repository does now

- **Builds run below interactive work.** Every `make` invocation lowers its own
  process to nice 19 and the idle I/O class at parse time, so every recipe,
  compiler, test binary, and Gradle or TLC JVM inherits that priority. Gradle
  also gets `--priority=low`, so a long-lived Gradle daemon started at normal
  priority is never reused for an agent-doc build. Set
  `AGENT_DOC_BUILD_PRIORITY=normal` to opt out. See
  [Building](building.md#build-priority-and-parallelism) for the knobs.
- **Parallelism leaves headroom.** Cargo, nextest, Gradle, and TLC sizes come
  from the cores that are actually idle (`cores - cores/8 - load1`). The result
  never drops below `max(2, cores/8)` and never exceeds `cores - cores/8`. Ten
  concurrent checks no longer each ask for 32 jobs.
- **The visual-highlight refresh no longer rebuilds on every pause.** The
  JetBrains plugin reconciles its range highlighters against the new tokens. It
  keeps every highlighter whose range, layer, kind, and style still match, so
  a pause in typing changes only the tokens that actually changed. Before, it
  removed and re-added every highlighter on the EDT, which meant hundreds of
  markup events and repaints on a long session document.

## What only the operator can change

These are machine or IDE settings. Apply the ones that fit; none is required
by agent-doc.

### 1. Undo the ananicy priority inversion

Create `/etc/ananicy.d/99-local.rules` (later files override earlier rules for
the same name), then `sudo systemctl restart ananicy-cpp`:

```json
{ "name": "idea", "nice": -4, "ioclass": "best-effort", "ionice": 2 }
{ "name": "tmux", "type": "IN_DIFF" }
{ "name": "tmux: server", "type": "IN_DIFF" }
{ "name": "rustc", "type": "BG_CPUIO" }
{ "name": "rust-lld", "type": "BG_CPUIO" }
{ "name": "clippy-driver", "type": "BG_CPUIO" }
{ "name": "cargo", "type": "BG_CPUIO" }
{ "name": "cargo-nextest", "type": "BG_CPUIO" }
{ "name": "sccache", "type": "BG_CPUIO" }
```

Verify with `ps -eo ni,comm | grep -E 'idea|tmux|rustc'`. The tmux server and
any shell started before the change keep their old nice value until they
restart. The Makefile wrapper covers agent-doc builds regardless.

### 2. Keep build and scratch trees out of IDEA's VFS

- **Settings → Editor → File Types → Ignored Files and Folders**: add
  `target`, `tmp`, `.tsift`, `.agent-doc`, `node_modules`, and `.gradle`.
  Ignored names never enter the VFS, so a new `target/` tree cannot trigger the
  recursive indexing walk that froze the EDT for 13 s.
- Create worktrees outside the opened project (for example in a sibling
  `<project>-worktrees/` directory), never under it.
- For directories you still want to browse, **Project Structure → Modules →
  Excluded** stops indexing but still has a VFS cost.

### 3. Reduce Git integration churn

- **Settings → Version Control → Directory Mappings**: unregister submodules
  you do not edit from this IDE. Each mapped root is refreshed on every ref
  change, and agents commit constantly. The IDE log also warns repeatedly about
  submodules that are not registered.
- **Settings → Version Control → Git**: turn off incoming/outgoing commit
  markers on branches and automatic fetching. These drive the repeated
  `git rev-list --count` forks.

### 4. Memory pressure

- Clear stale scratch trees from `/tmp`: old `agent-doc-*` checkouts with a
  `target/`, and leaked `.tmp*` fixture directories that no running process
  uses (`lsof +D` or check `/proc/*/cwd` first). Every GiB on tmpfs lives in RAM
  or in zram, and decompressing it costs CPU.
- `vm.swappiness` is 150 (zram default). Lowering it (for example
  `sysctl vm.swappiness=60`) keeps more of IDEA's working set resident.
- IDEA's `-Xmx16384m` is ample; heap use was 2.6 GiB. Do not raise it, because a
  larger heap only makes zram swap and G1 marking more expensive.

### 5. CPU power profile

The CPU governor is `powersave` with EPP `balance_performance`. For lower
wake-up latency while typing, set
`echo performance | sudo tee /sys/devices/system/cpu/cpu*/cpufreq/energy_performance_preference`
(not persistent; use `cpupower` or a power-profiles daemon to persist it).

### 6. Diagnose the next lag

- `top -H -p $(pgrep -x idea)` shows the hot IDEA threads, and
  `jstack $(pgrep -x idea)` (repeat 3-5 times) shows what they do.
- Freeze reports: `~/.cache/JetBrains/<product>/log/threadDumps-freeze-*`. Look
  at the `AWT-EventQueue-0` stack and grep it for `com.github.btakita.agentdoc`.
- agent-doc's own EDT timings: `grep 'crdt-perf' idea.log | grep AWT-EventQueue`.
