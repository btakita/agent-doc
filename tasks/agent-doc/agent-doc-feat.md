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
<!-- agent:boundary:68baa70f -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] [#6rm4] Bump version + release agent-doc with hooks feature + this fix
- [ ] [#mrs0] `agent-doc steer <FILE>` — inject steering message to claim pane via tmux send-keys
- [ ] [#qyv7] (stretch) inotify watcher for automatic real-time steering on document edit
- [ ] [#f8a4] (low priority) Port `find_code_ranges` into diff classifier for fenced-code-aware `#` handling
- [ ] [#2cyy] FFI library hot-reload: mtime check in `AgentDocLib.get()` → `Native.unregister` + reload (~30 lines Kotlin)
- [ ] [#wc5b] FFI library versioning: install as `libagent_doc-<version>.so` + `libagent_doc-current.so` symlink (Rust install + Makefile + PyPI wheel build)
- [ ] [#408p] `agent-doc gc-libs` subcommand: lockfile-tracked GC of stale versioned .so files (PID liveness via `/proc/<pid>`)
<!-- /agent:pending -->
