---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
agent_doc_model_tier: low
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
agent-doc v0.32.0+fix — Phase 1+2 guard gates deployed, binary fresh, ready for manual testing
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
## Summary (HEAD)

Diagnosed and fixed agent-doc submodule patch routing regression:
- **Root cause:** patches written to submodule's .agent-doc/patches/ (watched only by plugin in parent)
- **Fix:** 5 bug fixes implemented (git.rs, write.rs, tests)
  1. resolve_to_git_root() absolute-path branch now resolves to superproject
  2. resolve_ipc_project_root() helper ensures all IPC sites target parent patches dir
  3. Timeout fallback ipc_payload now includes normalize_prefix_lines, reposition_boundary, patch_id
  4. Timeout fallback writes content_ours to disk before snapshot/exit
  5. Regression test added for submodule patch routing
- **Stale binary issue:** fixes in source but running binary was 2h old (workflow change: new manual testing gate window with no auto-rebuild trigger)
- **Stale binary fix plan (Phase 1+2):** 
  - Phase 1: Added pre-preflight mtime guard in SKILL.md that auto-rebuilds if source newer
  - Phase 2: Hardened prerelease.md step 8 to require both make build-release AND cargo install before testing gate
- **Status:** All 5 fixes live in rebuilt binary, Phase 1+2 guards active
- **Next:** Manual testing gate on src/session-share/tasks/claudescore.md to verify ❯ preservation and working-tree consistency
<!-- agent:boundary:9b37d726 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- ✅ Bug 1–5: submodule routing fixes live in working tree + rebuilt binary
- ✅ **Stale-binary guard Phase 1+2 deployed** — skill guard + prerelease gate active
- 🆕 Manual testing gate: verify ❯ preservation + working-tree consistency on live src/session-share/tasks/claudescore.md
- → Commit the 04:58 source edits (git.rs, write.rs, Cargo.toml) once live-tested
- → Bump version, update VERSIONS.md, publish (crates/PyPI/GitHub Release)
<!-- /agent:pending -->
