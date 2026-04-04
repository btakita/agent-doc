---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
agent-doc v0.31.6 released to crates.io + GitHub
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary (compacted 2026-04-04 04:00)

**Release: agent-doc v0.31.6 + tmux-router v0.3.9**

**Bugs fixed (8):**
1. Console output spam — 24 task files contaminated, squashed 219 commits
2. Boundary (HEAD) marker stripping — concurrent preflight commits stripped markers
3. Stashed panes can't return — registry window pointed to stash
4. Claim not atomic — modified files before validating pane
5. Verbose git status in commit — filtered to only print commit result line
6. Window closure (last-pane guard) — stashed all panes including last
7. Navigation swap — 3 fixes: SessionScope cross-session, --window authority, path dep
8. Pre-response cleanup — clear_pending never deleted pre-response snapshots

**Features implemented (11):**
1. Debounce fix: 500ms → 2000ms default, configurable via `agent_doc_debounce` frontmatter
2. Structured logging: tracing crate, `AGENT_DOC_LOG=debug` env var
3. Lock file cleanup: Drop deletes file, stale lock detection (>1hr)
4. `agent-doc gc` subcommand: orphaned file cleanup (1,659 files cleaned, 62% reduction)
5. Auto-GC on preflight: daily via `.agent-doc/gc.stamp`
6. Cleanup runbook: `runbooks/cleanup.md`
7. Tracing instrumentation: sync, route, layout, resync modules
8. Source annotations: extract/transfer now include provenance blockquotes
9. Post-sync session health check: detects session destruction
10. Route cleanup on failure: kills orphaned panes on route error
11. Runbook convention: `## Runbooks` section in SKILL.md with trigger format

**Decisions:**
- Runbook trigger format: `` `trigger phrase` — runbooks/filename.md ``
- Section name: `## Runbooks` (not Operations)
- agent-runbooks: start as agent-doc subcommand, extract to crate if scope grows
- Corky has 3 extractable runbook candidates (tracked in tasks/runbooks.md)
- Claims log: still narrow (claim events only), hook system is the general IPC path
- Registry format: `HashMap<String, RegistryEntry>` (flat map, not array)

**tmux-router v0.3.9:**
- Cross-session pane operations allowed (SCOPE_CROSS warning, not SCOPE_BLOCK)
- Last-pane guard prevents window closure
- --window is authoritative for target session

**tmux crash investigation:**
- Session 5 destroyed, panes migrated to session 10
- Registry manually rebuilt (4 panes re-registered)
- Root cause: likely sync stashed all visible panes, closing last window
- Debug logging now enabled (`AGENT_DOC_LOG=debug` in .env)

**Test suite: 634 tests (193 lib + 403 bin + 38 integration)**

Release?
modified:   src/tmux-router (new commits, modified content)
### Re: Submodule pointer update

Both submodules have new commits that need their parent repo pointers updated:

- `src/tmux-router`: 8da141e → 9c8f13e (v0.3.9 release)
- `src/agent-doc`: fd69c0c → e145e79 (v0.31.6 release)

Both are already published to crates.io and pushed to GitHub. The parent `agent-loop` repo just needs its submodule pointers committed to track the new versions.

Should I commit the submodule pointer updates?
yes
### Re: Submodule pointers committed

Done. Parent repo now tracks:
- `src/agent-doc` → v0.31.6 (e145e79)
- `src/tmux-router` → v0.3.9 (9c8f13e)

Release?
### Re: Pushed (HEAD)

108 commits pushed to `origin/main` (37b6a2a7..0a96c915).

All releases complete:
- agent-doc v0.31.6 → crates.io + GitHub
- tmux-router v0.3.9 → crates.io + GitHub  
- agent-loop → pushed to GitHub (submodule pointers updated)
<!-- agent:boundary:8958cec4 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] All complete
<!-- /agent:pending -->
