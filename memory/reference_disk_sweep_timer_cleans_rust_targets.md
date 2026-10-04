---
name: reference_disk_sweep_timer_cleans_rust_targets
description: "bin/disk-sweep + systemd user timer auto-runs `cargo clean` on merged/idle worktree build dirs; check its log before hand-cleaning disk"
metadata:
  node_type: memory
  type: reference
  originSessionId: 9e9fe1f1-4d43-4bab-9c58-7f2ec089efec
  modified: 2026-10-04T17:56:56.488Z
---

`bin/disk-sweep` (agent-loop) runs every 10 min via `~/.config/systemd/user/disk-sweep.timer` (installed 2026-10-04). It only ever runs `cargo clean` on `target/` dirs under `~/work` (depth ≤6, needs `CACHEDIR.TAG` + sibling `Cargo.toml`); it never touches a dir with a running cargo/rustc/nextest process.

- **closed pass (always):** linked worktree, HEAD already in origin/main|main (HEAD == tip doesn't count), clean tree, idle ≥30m.
- **pressure pass:** free on `/` < 300G → clean most-idle first until 500G free; clean worktrees idle ≥60m, main checkouts or dirty worktrees idle ≥6h. Sends a `notify-send` if it can't get back above 300G.

Log: `~/.local/state/disk-sweep.log`. Preview: `bin/disk-sweep --dry-run --verbose`. Disable: `systemctl --user disable --now disk-sweep.timer`.

**Why:** parallel agent-doc sessions build ~55–160G per worktree and filled the 3.6T disk to 100% on 2026-10-04 (third refill in ~a day). Not covered: `/var/tmp` agent-doc test dirs (auto-mode classifier denied sweeping that shared scratch), `/tmp` tmpfs scratchpad targets (RAM, not disk), pacman cache (needs root).
