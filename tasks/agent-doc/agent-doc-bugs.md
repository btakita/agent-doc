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
### Re: release

v0.31.28 released:

- **Committed:** 7 files (Cargo.toml, pyproject.toml, VERSIONS.md, src/start.rs, src/stream.rs, src/template.rs, Cargo.lock) pushed to origin/main
- **crates.io:** `agent-doc v0.31.28` published
- **PyPI:** manylinux wheel uploaded (`agent_doc-0.31.28-py3-none-manylinux_2_17_x86_64.manylinux2014_x86_64.whl`)
- **Tag:** `v0.31.28` pushed — CI building GitHub Release
- **cargo install:** v0.31.28 running locally

❯ do
❯ - [ ] Update parent submodule pointer (git add src/agent-doc && git commit in agent-loop)
### Re: submodule pointer update

Done. `git add src/agent-doc && git commit` in agent-loop — pointer updated to v0.31.28 HEAD and pushed.

❯ Why does src/session-share/tasks/claudescore.md have an uncommitted response patch? What do the logs say?
❯ Did I call 2 agent-doc runs in a row? Which caused a race condition?
### Re: claudescore.md uncommitted patch — diagnosis

**What happened — `agent-doc write` without `agent-doc commit`:**

The `### Re: "Score pending..."` response was written by a previous session (`agent-doc write` ran, snapshot + file updated). But `agent-doc commit` was never called — snapshot had the content, git HEAD didn't. When THIS preflight ran at `10:38:31`, step 2 detected the gap and committed it at `10:38:43`.

**Evidence:**
- Git log: last real claudescore commit `09:48:31`, gap commit `10:38:43` (preflight step 2)
- Snapshot mtime: `10:38:43.593` — same timestamp as gap commit
- Diff content clean (52 insertions, no duplication) — no race, just a missed commit step

**2 agent-doc runs in a row?** Possibly — second session's preflight would commit the first's gap. But the diff is clean, so no overlapping writes. More likely: the session ended (Ctrl+C, /clear, timeout) after `agent-doc write` but before `agent-doc commit`.

**Not a bug** — preflight's gap commit is designed for exactly this. The `file is 176 bytes larger than snapshot` warning flagged it correctly. No data lost.

Why do `agent-doc write` + `agent-doc commit` need to be separate? Can be part of the same agent-doc operation instead? 
### Re: merge write + commit

**Why they're currently separate:**

`agent-doc write` handles file write + snapshot update + IPC to the plugin. `agent-doc commit` handles git staging via `hash-object` + `update-index` + `(HEAD)` boundary marker. They're separate because streaming checkpoints need write-without-commit — intermediate flushes during a long response shouldn't create partial git commits.

**Yes, they can be merged with a `--commit` flag:**

The commit step runs atomically after write completes — including after the IPC ACK-content sidecar is read. Streaming checkpoint writes omit `--commit`; only the final write uses it.

**This eliminates the gap-commit class entirely.** The window between write and commit disappears. The skill's step 3 becomes one command instead of two.

**Implementation:**
1. Add `--commit` bool flag to the `write` subcommand
2. After `run_stream` returns successfully, call `commit::run(file, opts)`
3. Update SKILL.md step 3 to use `--commit` on the final write (checkpoint writes stay as-is)

❯ Should we have `agent-doc write` do `--commit` by default? Can it check if we are in git repo? If not, ignore the commit?
### Re: commit-by-default? (HEAD)

**The streaming checkpoint problem is the blocker.** Template mode sessions flush partial responses mid-response. Commit-by-default would create a git commit per checkpoint — noisy and wrong. The skill would need `--no-commit` on every checkpoint write, which is more friction, not less.

**Keep `--commit` as opt-in.** The skill uses it on the final write; checkpoints omit it.

**Git repo check:** Yes — `commit::run` gates on `git rev-parse --is-inside-work-tree`. If not in a repo, logs `[commit] skipped (not in git repo)` and returns `Ok(())`. Makes `--commit` safe everywhere without call-site error handling.

**Proposed behavior:**
- `agent-doc write` — no commit (safe for checkpoints and non-git dirs)
- `agent-doc write --commit` — commit after write; silently skips if not in git repo
- Skill step 3: final write always uses `--commit`; checkpoint writes do not
<!-- agent:boundary:4317107c -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Implement `agent-doc write --commit` flag + git-repo guard (awaiting approval)
<!-- /agent:pending -->
