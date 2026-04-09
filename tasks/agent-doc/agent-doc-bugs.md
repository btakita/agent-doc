---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
agent-doc v0.31.26 — released
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Sessions v0.31.18–v0.31.26. Key decisions and fixes:

**v0.31.18–v0.31.24 (prior summary):**
1. Cross-session swap-pane bug (rescue_from_stash guard), slash command dispatch, partial compact, JB dedup guard.
2. PyPI trusted publisher via GitHub Actions.
3. ❯ prefix normalization (binary-side, append+replace modes); overeager prefix bug fixed (diff snapshot→baseline for true user additions).
4. Release gate rule: pause after cargo install for user confirmation before publishing.
5. Slash command routing: builtin_commands vs slash_commands in preflight JSON. Plugin autocomplete.
6. Git archive (v0.31.23): log/show/diff subcommands; compact auto-tags pre-compact commits.
7. fullContent+patches conflict fixed: only send fullContent when no component patches present.
8. Last-boundary normalization: use LAST boundary for user region detection.
9. normalize_prefix_lines HashSet bug: positional zip comparison.
10. Fence-interior ❯ bug fixed; ~~~ tilde fence gap fixed.
11. Plugin version bump rule: any plugin change requires version bump.
12. Commit-after-write rule: always call agent-doc commit after every agent-doc write.

**v0.31.25 — Token efficiency + snapshot correctness:**
- preflight now diff-only by default: `document` field removed from PreflightOutput entirely. `agent-doc read <FILE>` subcommand added for on-demand HEAD fetch. SKILL.md updated: first cycle only runs `agent-doc read`.
- Stale `content_ours` snapshot bug fixed: all IPC paths (socket + file IPC) now save actual post-write disk state (200ms flush + file read) instead of `content_ours`. Non-IPC paths save `final_content`. Hard guarantee for non-IPC; soft guarantee (reconciles on next cycle) for IPC.
- Ghost-reappearance root cause: `agent-doc commit` stages snapshot content, not file content — when snapshot ≠ file, gap commit regresses HEAD. Fix: snapshot = disk state eliminates gap.
- Stash window non-idle check removed (false positives for intentional backgrounded sessions).
- Duplicate claims check added to `check_layout()` with 4 tests.
- `apply_stream_from_string` dead code removed.
- Preflight token-efficiency tests: `preflight_output_no_document_field`, `preflight_output_no_large_content`.
- v0.31.25 released (crates.io + PyPI + GitHub Release).

**v0.31.26 — IPC improvements + recovery dedup:**
- IPC idempotent apply: `patch_id` UUID added to all 3 IPC payload sites. `--force-disk` writes sentinel to `.agent-doc/claimed-patches/<patch_id>` before deleting stale patch. Plugin checks sentinel in both `processPatchFile` and `handleSocketMessage`. `isPatchAlreadyApplied` moved inside `invokeLater` (closes timing race).
- ACK-with-content sidecar: after successful apply, plugin calls `agent_doc_write_ack_content` FFI → writes `.agent-doc/ack-content/<patch_id>.md`. Binary reads sidecar after ACK; if present uses as snapshot, deletes sidecar. Falls back to 200ms + disk read for older plugins.
- FFI exports: `agent_doc_write_ack_content`, `agent_doc_is_claimed_by_force_disk` (both in `ffi.rs`).
- JB plugin v0.2.54: `IpcPatch.patchId`, `writeAckContent`, `isClaimedByForceDisk` via JNA, `isPatchAlreadyApplied` dedup inside EDT invokeLater.
- Recovery dedup: `recover::run` checks response fingerprint (first 3 non-empty non-marker lines) against document before re-applying. `recover_skips_duplicate_apply` test added.
- `cargo install` → v0.31.26 running locally.

**Open / Pending:**
- Manual verification: confirm ghost-reappearance fixed in next /agent-doc cycle.
- Install plugin v0.2.54 (full uninstall+restart for class structure changes, per runbooks/plugin-install.md).
- Publish v0.31.26: crates.io (`cargo publish`), PyPI (`maturin publish --no-sdist`), GitHub Release (`gh release create v0.31.26 --generate-notes` + prebuilt binary).
- Plan file: `tasks/agent-doc/plan-ipc-improvements.md`.

**Architectural rules established:**
- Snapshot invariant: always save actual post-write disk state, not `content_ours` from baseline.
- FFI-first: new socket/IPC behavior implemented in ffi.rs, called from plugins via JNA.
- Runbook: `runbooks/plugin-install.md` must be consulted for any JB plugin change (version bump, uninstall protocol).

❯ release
### Re: release

v0.31.26 released:

- **Committed:** 7 files (recover.rs, preflight.rs, Cargo.toml, pyproject.toml, VERSIONS.md, SKILL.md, Cargo.lock) pushed to origin/main
- **crates.io:** `agent-doc v0.31.26` published
- **PyPI:** manylinux wheel built and uploaded (`agent_doc-0.31.26-py3-none-manylinux_2_17_x86_64.manylinux2014_x86_64.whl`)
- **Tag:** `v0.31.26` pushed — CI building GitHub Release with cross-platform binaries
- **cargo install:** v0.31.26 running locally

❯ The agent-loop project is swapping with tmux session 1. We should keep everything on this tmux session 0.
❯ This has been a reoccuring issue. Let's figure out all of the horizontal and vertical workflows that can cause this.
### Re: session drift — tmux session 1 contamination (HEAD)

**Current state:** `%57` (mattpocock.md) is registered in `$1:@13` (session 1). All other panes are in session 0. Config correctly shows `tmux_session = "0"`.

**Root causes — what creates cross-session registrations:**

1. **Start from wrong terminal** (`start.rs`): `sessions::current_pane()` reads `$TMUX_PANE`. If `/agent-doc` runs from a Claude Code terminal attached to session 1, that pane registers to session 1. This is the most likely trigger for mattpocock.md.

2. **`resolve_target_session` fallback + auto-update** (`route.rs:473`): When the configured session is dead or unset, it falls back to `current_tmux_session()` and **writes that back to `config.toml`**. A single route call from a session 1 context silently flips `tmux_session = "1"`, causing all subsequent routing to land there.

3. **`break_pane` fallback in `stash_pane`** (`tmux-router/src/tmux.rs:717`): When `ensure_stash_window(session_0)` fails, it falls back to `break_pane` with no `-t` destination. tmux sends the pane to its current session — which could be session 1 if the caller was cross-session.

4. **`sync.rs` / `parallel.rs` stash with wrong `target_sess`**: If `target_sess` resolves to "1" via the route fallback, stash operations accumulate panes in session 1's stash window.

**Fix path:**

`resync.rs` currently only detects `WrongSession` when `frontmatter.tmux_session` is explicitly set. It should also check registered panes against `config::project_tmux_session()`. Panes in session 1 (while config says session 0) would trigger `WrongSession` → auto-relocated via `join-pane` to session 0. This makes the system self-healing without requiring `tmux_session` frontmatter in every document.

**Immediate action:** Manually relocate `%57` now, or just kill+deregister mattpocock.md and re-run it from a session 0 terminal.
<!-- agent:boundary:9ec95bd4 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Fix `resync.rs`: detect `WrongSession` against `config::project_tmux_session()` (not only `frontmatter.tmux_session`)
- [ ] Guard `resolve_target_session` auto-update: only write config when new session matches `project_tmux_session` (don't let session 1 overwrite session 0 config)
- [ ] Relocate or deregister `%57` (mattpocock.md) — it's in session 1, should be in session 0
<!-- /agent:pending -->
