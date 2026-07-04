---
name: feedback_forcedisk_editorless_scramble_reset_fromcurrent_repair
description: "On editor-less (claude-code pane) docs, --force-disk CRDT-merges and resurrects operator-deleted queue items; repair via edit + git commit + reset --from-current --force-disk; halt no-op churn with queue marker start→stop."
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 5bf7dfbd-7742-4e67-a4ae-f08688e29f77
---

On an **editor-less** agent-doc doc (admin inspect shows `[claude-code] pane=%N`, no JetBrains/VS Code process), a `finalize --force-disk` recovery does a CRDT merge ("File was modified during response generation. CRDT merging...") that **resurrects operator-deleted managed-region content** — e.g. queue items the operator removed came back (a paused-triggering `/goal #zhfn` head, an old prompt), plus duplicates. Verified 2026-07-02 on `tasks/professional/equityfundingsource.md`.

**Why:** force-disk merges base(HEAD) + ours(patch) + theirs(disk); when HEAD still holds the pre-edit managed region, the union re-adds deleted lines. This is operator-visible text drift caused by the recovery itself.

**How to apply:**
- Prefer the normal editor/CRDT path. Only use `--force-disk` when there is genuinely no live editor AND you accept a possible merge; then **verify the managed region after** (`awk '/agent:queue/../\/agent:queue/'`).
- To repair a scramble, restore operator intent by direct **Edit** on disk, then `git add + git commit`, then `agent-doc reset --from-current --preserve-session --force-disk <FILE>` to realign snapshot/CRDT base to the corrected disk. This ran clean (no re-scramble) repeatedly; `reset --from-current` makes disk authoritative so no merge happens. See [[feedback_stale_binary_crdt_deadlock_direct_write_git_commit_reset]].
- To end **no-op `/agent-doc` re-trigger churn** on an active-but-paused queue, flip the queue component marker `start` → `stop` (e.g. `<!-- agent:queue preset="..." stop -->`), commit, reset — gives `queue_active:false`. Frontmatter `queue: stop` alone only sets `auto=false`; the component marker governs `queue_active`. `admin queue pause` needs an observed generation and rejected without it. Struck/kept items are preserved under a stopped marker. [[feedback_go_marker_churn_pause_resets_remove_go_set_stop]]
