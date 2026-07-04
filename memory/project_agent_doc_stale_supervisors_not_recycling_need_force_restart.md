---
name: project_agent_doc_stale_supervisors_not_recycling_need_force_restart
description: "restart-supervisor --all/--project + supervisor-status built (branch restart-supervisor-all); the \"all supervisors stale\" alarm was a measurement artifact, NOT real staleness"
metadata: 
  node_type: memory
  type: project
  originSessionId: 66297cbc-5f05-428d-b1bd-51619e77442a
---

Operator's #1 bugs2 ask was a forceful bulk supervisor restart + "am I up to date in tmux" visibility. **Built 2026-06-26** in worktree `src/agent-doc-rsall-wt` (branch `restart-supervisor-all`, commit `8aa96bfd`, make check green / 5413 tests):
- `agent-doc session restart-supervisor --all` (all projects) / `--project` (current project), inline staleness flags, **skips this session's own host supervisor** (ancestor walk).
- `agent-doc session supervisor-status [--all|--project] [FILE]` — read-only inode-based stale inspector (compares running supervisor `/proc/<pid>/exe` inode vs the inode of the agent-doc binary *that invokes the command*).

NOT installed/pushed from session (installing recycles the live host mid-drain). Operator step to ship: review/merge `restart-supervisor-all` → `make install-full`.

**CORRECTION (do not repeat my earlier mistake):** I initially concluded "all supervisors are STALE and not recycling onto installed fixes." That was WRONG — a measurement artifact. `supervisor-status` compares against `current_exe()`, and the subagent ran it from the **worktree** (0.34.50 debug build), so every supervisor on the real installed binary looked "older." Against the actual installed binary the supervisors are FRESH. Verified: installed `agent-doc 0.34.49` == source HEAD `06c84d99` (binary mtime 13:02:53 ≈ HEAD commit 13:02:39); bugs2 host supervisor pid started 13:04:44 > install; `admin inspect` = `supervisor:fresh`. **Before claiming a supervisor is stale, compare its start time / `/proc/exe` against the INSTALLED binary (`command -v agent-doc`), never a worktree debug build, and cross-check `agent-doc admin inspect`.** See [[feedback_stale_binary_supervisor_causes_live_crdt_edit_bugs]] for the real-staleness diagnosis (ps lstart vs INSTALLED binary mtime).
