---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---


## Status

<!-- agent:status patch=replace -->
✅ agent-doc v0.32.1 — env frontmatter wired into all Claude-spawning paths (start / run / stream / parallel). Build clean, 42 targeted tests passing. Ready for live test.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
**v0.33.x bug sessions — 2026-04-19**

**Bugs fixed in 0.33.2:**
- Stream replace truncation: --stream path now uses append
- Enter/q stdin not submitted after session restart: supervisor stdin wiring fixed
- Submodule path resolution: JB/VS Code plugins resolve paths relative to agent-doc project root
- is_busy guard blocked tmux-send (0.2.61)
- IPC auto-degradation (#d2d4): 500ms→1000ms timeout, repeated failures degrade to disk
- IPC timeout pending mutation: upgraded to CRDT merge in timeout path

**Bugs fixed in 0.33.3:**
- Orphan recovery dedup: fingerprint_lines prevents duplicate patchback
- IPC fallback patch file race (write.rs): pre-write fallback before socket send; sidecar timeout → success:false → disk fallback
- IDE buffer overwrite (#s80z): reposition handler was missing reloadFromDisk before repositioning

**Bugs fixed in 0.33.4:**
- PromptPoller.autoSaveTrackedFiles() (#mpjv): stale buffer overwrote CRDT-merged content 51s post-commit. Fixed with VFS refresh + modstamp check before auto-save. BulkFileListener added for sub-1.5s reactive merge
- Resync kills active agents (#2ea0): WrongSession handler now join-pane relocates active AGENT_PROCESSES instead of killing
- PluginLifecycleListener --session flag (#8ndp): passes current tmux session to resync so all panes get relocation mode
- Config.toml auto-update: resolve_target_session rewrites tmux_session when configured session is stale

**Bugs fixed in 0.33.5:**
- SIGSEGV PC=0x0 on cargo install (#mpjv IDEA crash): cargo install overwrote mmap'd .so. Fixed with: (1) mtime-based FFI reload in AgentDocLib.get(), (2) versioned cdylib install (libagent_doc-<version>.so + libagent_doc.so symlink via atomic rename), (3) lockfile-tracked GC (agent-doc gc-libs, JVM shutdown hook, VS Code deactivate())

**Decisions:**
- IPC timeout → CRDT merge (not atomic_write) — preserves all concurrent changes
- mtime-based FFI reload + versioned cdylib + lockfile GC as .so overwrite fix (not pure Option 3 nor pure Option 4 — hybrid)
- Promotion heuristic: agent proposals at end of response → --pending-add same cycle (added to pending-ops.md)
- skill install does NOT sync runbooks/ — only SKILL.md propagates. #y2gr filed
- No local maturin publish — PyPI flows through release:published CI automatically
- Config.toml auto-updates tmux_session when configured session dies
- Build on every version bump — enforced

**v0.33.5 status at compact:**
- Pre-release checks: 1158 tests pass, release binary built, cargo install done, JB plugin 0.2.67 built
- **At manual testing gate — NOT yet published to crates.io or GitHub**
- Required: verify IDEA does not crash when cargo install runs during active session; new lib version logged
<!-- agent:boundary:9e3ac672 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [/] [#a002] Safety rail + forensic log in normalize_user_prompts_in_exchange — ✅ landed, awaiting large-drift trip for `normalize_threshold_exceeded` telemetry.
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Ship with multi-user support, not before. Tracked: https://github.com/btakita/agent-doc/issues/40
- [ ] [#066r] Test on Opus 4.7 — baseline current behavior before prompt changes, then iterate
- [/] [#y2gr] Fix agent-doc skill install: does not sync runbooks/*.md to ~/.claude/skills/agent-doc/runbooks/ or <project>/.claude/skills/agent-doc/runbooks/ — only SKILL.md propagates. Had to cp manually today.
<!-- /agent:pending -->
## Assistant
