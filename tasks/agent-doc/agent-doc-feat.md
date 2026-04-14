---
agent_doc_session: 09760345-ef9d-426a-9416-250563e42548
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Implemented document-level hooks (session_start/post_write/post_commit) via frontmatter YAML. Fixed (HEAD) suffix incorrectly committed to fenced code # comments (is_fence_marker + in_fence tracking in git.rs). Fixed ❯ prefix lost in editor after transfer (normalize_patch_content bakes prefix into IPC patch body). Discussed real-time steering via tmux send-keys. Open: release, agent-doc steer command, inotify watcher, fenced-code diff classifier.
<!-- agent:boundary:b3ee89fc -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Bump version + release agent-doc with hooks feature + this fix
- [ ] `agent-doc steer <FILE>` — inject steering message to claim pane via tmux send-keys
- [ ] (stretch) inotify watcher for automatic real-time steering on document edit
- [ ] (low priority) Port `find_code_ranges` into diff classifier for fenced-code-aware `#` handling
<!-- /agent:pending -->
