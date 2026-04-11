---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
agent-doc v0.31.29 — released
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Sessions v0.31.18–v0.31.31 + IDE comparison. Key decisions and fixes:

**v0.31.18–v0.31.26 (prior):** Cross-session swap-pane bug; slash command dispatch; partial compact; JB dedup guard; IPC idempotent apply (patch_id UUID + ACK-content sidecar); snapshot=disk-state ghost-reappearance fix; v0.31.26 released.

**v0.31.27 — pane_policy + session drift hardening:** PaneMoveOp + CrossSession enum; 22 call sites migrated (deny by default, 3 explicit allow); start.rs + route.rs + resync.rs session guards; v0.31.27 released.

**v0.31.28 — exchange dedup + auto-relocate:** apply_mode(append) echoed user prompt; fix via dedup_exchange_adjacent_lines + strip_leading_overlap; start.rs auto-relocate via PaneMoveOp (warn→move); v0.31.28 released.

**v0.31.29 — write --commit flag:** agent-doc write --commit runs git commit after write (atomically), eliminating the write/commit gap class. git::is_in_git_repo guard. SKILL.md step 2 updated to use --commit on final write; step 3 removed. v0.31.29 released.

**v0.31.30 — normalize_patch_content scope fix + claim position:** normalize_patch_content gated on is_append_mode_component — replace-mode patches (pending, status) no longer receive ❯ prefix. JB plugin v0.2.55: ClaimAction passes --position left|right from detectEditorLayout, fixing wrong-pane swap on claim. FFI boundary decision: editor layout detection stays in plugin (IntelliJ Swing APIs), --position flag is the shared binary interface. v0.31.30 released.

**v0.31.31 — commit-reliability (5 fixes) + HEAD fenced block + CWD anchor:**
- Fix 1: snapshot + git commit before exit(75) in write.rs (IPC timeout path)
- Fix 2: commit before result? propagation in main.rs
- Fix 3: retry on git index.lock contention (3x backoff: 100/200/400ms)
- Fix 4: agent_doc_commit FFI export; NativeLib.kt + PatchWatcher.kt call it post-apply
- Fix 5: preflight cross-document sweep — scans all tracked docs, commits any where snapshot mtime > file mtime
- HEAD fenced block bug: pulldown-cmark AST replaces ad-hoc is_fence_marker across 5 code paths in boundary.rs. Prevents (HEAD) markers inside fenced code blocks. Triggering case: nested ```bash inside plain ``` fence.
- project_config_path(): walk-up from CWD to find .agent-doc/, avoiding CWD-sensitivity in submodule sessions
- v0.31.31 released (crates.io + PyPI + GitHub Release)

**IDE/Editor options for AWS server (SSH/tmux, no JetBrains):**
Compared neovim, vim, zed, vscode (code-server), cursor, for agent-doc integration (IPC socket + git gutter refresh).
- neovim: recommended — SSH native, gitsigns.nvim, Lua plugin ~3–5 days
- vim: minimal overhead, no built-in LSP, agent-doc plugin ~5–7 days (harder ecosystem)
- zed: remote SSH native (beta), Rust extension API, ~5–7 days; GPU dependency may be an issue on headless server
- vscode (code-server): most familiar, git SCM built-in, TypeScript extension ~2–3 days; high memory (~300MB+)
- cursor: fork of vscode, same integration path as vscode; subscription cost; ~2–3 days
- No 'antigravity' or 'ki' found in research — likely not real or too niche

**Open items:**
- /model haiku: switch model to haiku (run /model haiku in the terminal — cannot execute Claude Code built-ins from within the session)
- IDE integration: decision pending on which editor to implement next for AWS server use
<!-- agent:boundary:3947f7fa -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->

<!-- /agent:pending -->
