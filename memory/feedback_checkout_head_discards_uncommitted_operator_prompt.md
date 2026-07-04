---
name: feedback_checkout_head_discards_uncommitted_operator_prompt
description: "During agent-doc cycle recovery, `git checkout HEAD -- <doc>` discards the operator's uncommitted prompt (editor-buffer text never committed) — preserve/restore it"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: d845d3b3-1d66-43fb-9c53-5fa524744686
---

When recovering a wedged agent-doc finalize cycle, `git checkout HEAD -- <session-doc>` reverts the doc to the committed tree — which **discards the operator's just-typed prompt** if it only ever existed in the editor buffer / uncommitted working tree (a fresh prompt is usually NOT yet in HEAD). This drops operator-visible text, violating the "operator text is authoritative" rule.

**Why:** the operator's prompt arrives as an editor-buffer edit; preflight may not have committed it before the cycle wedged, so it lives only in the working tree.

**How to apply:** before `git checkout HEAD -- <doc>` in recovery, grep the doc for the operator's prompt and confirm it is in HEAD (`git show HEAD:<doc> | grep`). If it is uncommitted, capture the exact text first (it's preserved in the `/agent-doc` invocation context / conversation) and splice it back into `exchange` before the `### Re:` response when reconstructing. Inserting a missing user prompt via direct edit is explicitly sanctioned by the skill.

Recovery chain seen 2026-07-01 on bugs2 (dead JB editor, session b26b9957): finalize IPC `no_ack` (dead editor) → force-disk tripped the false-positive `guard_visible_write_idle_current_or_target` from a stale `.agent-doc/op-capture/<hash>.json` (my own attempt's residue, stale base_hash) → moved op-capture aside → git-direct splice of prompt+response → commit → `reset --from-current --preserve-session` → session-check ok. Related: [[feedback_noack_file_ipc_patch_drops_content_recover_from_patch_json]], [[feedback_no_ack_write_socket_finalize_fails_writecommit_recovers]], [[feedback_complete_crashed_agent_doc_run_recovery_sequence]].
