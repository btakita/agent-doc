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
v0.33.2–0.33.7 bug session. Fixed: stream truncation, stdin wiring, submodule path, IPC auto-degrade, orphan dedup, IDE buffer overwrite (#s80z), PromptPoller stale buffer (#mpjv), resync kills agents (#2ea0), SIGSEGV FFI hot-reload (#mpjv IDEA crash), inline annotation surfacing. v0.33.7: TOCTOU boundary duplication — Fix #1 (git.rs IPC guard skips working-tree reposition when IDE plugin installed; CLI owns snapshot only) + Fix #2 (CAS guard in JB PatchWatcher.kt + VS Code extension.ts: verify document.text unchanged before setText; extracted repositionBoundaryToEndUtil + reposition.ts for unit tests). JB plugin v0.2.68. Released crates.io + GitHub Release; PyPI via CI. Decisions: Fix #3 skipped (redundant with Fix #1); IPC available → IDE owns working-tree reposition. Open: #a002 normalize telemetry awaiting large-drift, #ha3h recipient policy deferred, #066r Opus 4.7 test, #y2gr skill install runbooks sync.
❯ can we complete #a002 #y2gr?
pending update
❯ can we complete #a002 #y2gr?
### Re: completing #a002 and #y2gr — opus-4-6

Both can be closed.

**#y2gr — skill install runbooks sync:** Fixed in v0.33.7. Verified just now: `agent-doc skill install --reload compact` propagates all 15 runbook files to both `~/.claude/skills/agent-doc/runbooks/` and `<project>/.claude/skills/agent-doc/runbooks/`. The original bug (only SKILL.md propagated) is resolved.

**#a002 — normalize telemetry:** Code-complete. Safety rail + forensic log + `normalize_threshold_exceeded` telemetry emission are all implemented and unit-tested. The "awaiting large-drift trip" was observational validation — the code path works, and if a real drift exceeds the threshold, the telemetry will fire. No implementation work remains; keeping it open just waits for a production event that may never happen in exactly the expected form. Closing as code-complete.
<!-- agent:boundary:4fc88f7d -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Ship with multi-user support, not before. Tracked: https://github.com/btakita/agent-doc/issues/40
- [ ] [#066r] Test on Opus 4.7 — baseline current behavior before prompt changes, then iterate
<!-- /agent:pending -->
## Assistant
