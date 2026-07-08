---
name: reference_bin_agent_doc_symlink_is_reporoot_relative
description: ".bin/agent-doc symlink target is intentionally repo-root-relative (src/...), NOT ../src/... — do not \"fix\" it to match siblings"
metadata: 
  node_type: memory
  type: reference
  originSessionId: 249fae54-7a60-4c70-bf2a-2353758d3c82
---

`.bin/agent-doc` → `src/agent-doc/target/release/agent-doc` (relative string, resolves from **repo root**, not from `.bin/`). This looks broken versus siblings `.bin/corky`/`.bin/chromium-bridge` which use `../src/...`, and shell invocation of `.bin/agent-doc` fails ("no such file") because the shell resolves the link from `.bin/`. But agent-doc's **project-controller launch reads the symlink target string and execs it with cwd = repo root**, where `src/agent-doc/...` exists. CLAUDE.local.md documents the target as `src/agent-doc/target/release/agent-doc` for exactly this reason.

**Trap:** "fixing" it to `../src/agent-doc/...` (to match siblings / make shell invocation work) breaks `agent-doc admin inspect|commit|...` on any active-actor doc with `Error: failed to launch project controller / No such file or directory (os error 2)` — because `../src/...` from repo root resolves to `/home/brian/work/btakita/src/...` (nonexistent). Docs with no active actor still work, masking the break.

**How to apply:** leave `.bin/agent-doc` pointing at `src/agent-doc/target/release/agent-doc`. To invoke agent-doc from a shell, use the cargo-installed `agent-doc` on PATH, not `.bin/agent-doc`. If os-error-2 controller-launch appears, check `readlink .bin/agent-doc` first. See [[feedback_turn_unfinalized_live_editor_authority_blocks_commit_forcedisk_gitcommit]].
