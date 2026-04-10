---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
agent-doc v0.31.28 — released
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Sessions v0.31.18–v0.31.28. Key decisions and fixes:

**v0.31.18–v0.31.26 (prior summary):**
1. Cross-session swap-pane bug, slash command dispatch, partial compact, JB dedup guard.
2. PyPI trusted publisher via GitHub Actions.
3. ❯ prefix normalization (binary-side); overeager prefix bug fixed.
4. Token efficiency: preflight diff-only, `agent-doc read` subcommand added.
5. Stale `content_ours` snapshot bug fixed: all IPC paths save actual post-write disk state.
6. Ghost-reappearance root cause: snapshot = disk state eliminates gap.
7. IPC idempotent apply: `patch_id` UUID; ACK-with-content sidecar via `agent_doc_write_ack_content` FFI.
8. JB plugin v0.2.54: writeAckContent, isClaimedByForceDisk, isPatchAlreadyApplied dedup inside EDT invokeLater.
9. Recovery dedup: fingerprint check before re-applying.
10. v0.31.26 released (crates.io + PyPI + GitHub Release).

**v0.31.27 — pane_policy + session drift hardening:**
- Root causes: start from wrong terminal, `resolve_target_session` auto-update, 22 unchecked pane move sites.
- `tmux-router/src/pane_policy.rs`: PaneMoveOp + CrossSession enum; 22 call sites migrated (deny by default, explicit allow for 3 cross-session sites).
- `start.rs` guard: warn when TMUX_PANE session ≠ project_tmux_session.
- `route.rs`: only write session config when none previously configured.
- `resync.rs`: WrongSession detection uses config::project_tmux_session() fallback.
- Validated: agent-doc resync --fix --session 0 relocated %57 session 1 → session 0.
- v0.31.27 released (crates.io + PyPI + GitHub Release).

**v0.31.28 — exchange dedup + auto-relocate:**
- Exchange duplication bug (mise.md): apply_mode(append) was echoing user prompt in patch.
- Fix 1: dedup_exchange_adjacent_lines in template.rs — removes consecutive identical non-blank lines after patch apply.
- Fix 2: strip_leading_overlap in apply_mode(append) — strips patch prefix duplicating last existing line.
- start.rs warn → auto-relocate via PaneMoveOp: pane in wrong session moved before registration.
- relocate_if_wrong_session helper extracted with 3 tests.
- 765 tests pass; cargo install v0.31.28 running locally.

**Open / Pending:**
- Manual testing gate: confirm v0.31.28 auto-relocate in live session, then publish: crates.io → PyPI → GitHub Release.

❯ release
### Re: release (HEAD)

v0.31.28 released:

- **Committed:** 7 files (Cargo.toml, pyproject.toml, VERSIONS.md, src/start.rs, src/stream.rs, src/template.rs, Cargo.lock) pushed to origin/main
- **crates.io:** `agent-doc v0.31.28` published
- **PyPI:** manylinux wheel uploaded (`agent_doc-0.31.28-py3-none-manylinux_2_17_x86_64.manylinux2014_x86_64.whl`)
- **Tag:** `v0.31.28` pushed — CI building GitHub Release
- **cargo install:** v0.31.28 running locally
<!-- agent:boundary:d27a4fda -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Update parent submodule pointer (git add src/agent-doc && git commit in agent-loop)
<!-- /agent:pending -->
