---
name: feedback_firstcycle_ipc_dedupe_no_head_prompt_recovery
description: "First-cycle prompt_duplication_in_ack_content (prompt not yet in HEAD) — can't checkout HEAD; rebuild exchange + move stale cycle aside + stash/reset/pop + finalize --force-disk"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: bfc2b4c9-1693-40c9-9702-795acb929641
---

On the **first response cycle** of a fresh agent-doc doc, a degraded `finalize` (`invariant=prompt_duplication_in_ack_content` → `ipc_dedupe` → `recovery=retry_without_disk_write redeliver_editor=true`) mangles the response in the working tree (strips `### Re:` headings + prose, keeps only list lines, `❯`-prefixes the prompts below) and refuses the direct disk write. The full correct body is in `content_ours` (the larger byte count).

**Why the usual recovery doesn't apply here:** the standard force-disk recovery ([[feedback_ipc_crdt_response_loss_forcedisk_recovery]]) starts with `git checkout HEAD -- FILE` — but on the first cycle the **user prompt is not yet in HEAD** (HEAD is the pre-prompt doc), so checkout would DROP the prompt. Also the retained `response_captured` cycle + CRDT overlay keep **replaying the mangled fragments** into the working tree after every clean Edit, deadlocking `write --commit`/`reset` with `captured response baseline no longer matches current document`.

**How to apply (recovery sequence that worked, adduro.md 2026-06-26):**
1. Edit the exchange back to the clean pre-response state (user prompts + boundary only, no mangled response, no `❯`).
2. Move the stale cycle aside so nothing replays: `mv` `.agent-doc/captures/cycle-<runid>.json`, `.agent-doc/state/cycles/<pathhash>.json`, and `.agent-doc/crdt/<pathhash>.{nodes,overlay,}.yrs{,.lock}` to /tmp. (cf [[feedback_supervisor_crdt_replay_reset_from_current]], [[feedback_agent_doc_stale_capture_blocks_commit]].)
3. `agent-doc reset --from-current <FILE>` (no `--preserve-session`) to rebuild CRDT from the clean file and drop the stuck cycle.
4. That trips the `snapshot differs from HEAD without an open or recoverable cycle` guard ([[feedback_snapshot_vs_head_guard_stash_reset_to_head]]) → `git stash push <FILE>` → `reset --from-current --preserve-session` (snapshot==HEAD) → `git stash pop` so the prompt becomes a normal diff.
5. `agent-doc preflight` now detects the prompt as a fresh cycle. Persist with `finalize --force-disk` (NOT the socket — it re-dedupes) → committed + session-check OK.

The controller socket was also unresponsive (`timed out waiting for project controller at .agent-doc/controller.sock`) — a fully degraded session; force-disk is the right path. Don't retry the socket finalize (duplicates).
