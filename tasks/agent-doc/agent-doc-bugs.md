---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
agent-doc v0.31.14 released to crates.io + GitHub
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->

### Exchange transferred

Architecture discussion transferred to `tasks/software/instruction-files.md` via `agent-doc transfer`. This document's exchange is now clean for new agent-doc bug reports.

**Session shipped:** agent-doc v0.31.13 (released) + v0.31.14 changes (claim fix, sync auto-scaffold, 456 tests).
When we transferred to tasks/software/instruction-files.md, we should also add to the agent:pending
Can this be deterministic + agentic?
### Re: Transfer should update pending + can it be deterministic?

**1. Transfer should update pending in the target**

Yes. When `agent-doc transfer` moves exchange content, the target's pending component should reflect the transferred items. Currently transfer only moves the exchange component — it doesn't touch pending.

The fix: `agent-doc transfer` should accept an optional `--pending` flag (or always transfer pending alongside exchange). The binary would:
1. Move exchange content (existing behavior)
2. Also move/merge pending items from source to target
3. Clear source pending after transfer

This is deterministic — the binary reads both pending components, merges them, writes the result.

**2. Can this be deterministic + agentic?**

Yes. The transfer operation itself is fully deterministic (binary handles file I/O, component parsing, git commit). The agentic part is the decision to transfer — the skill decides *when* and *what* to transfer based on user input.

The current split is correct per the binary/skill responsibility table:
- **Binary** (deterministic): parse components, move content, merge pending, save snapshots, commit
- **Skill** (agentic): decide to transfer, choose source/target/component

Adding pending transfer to the binary keeps it deterministic. No LLM judgment needed — just merge the pending lists.

This is a v0.31.14 feature for the agent-doc binary. Want me to implement it?
yes
### Re: Pending transfer implemented

Done. `agent-doc transfer` now automatically transfers the pending component alongside the named component.

**What changed in `extract.rs`:**
- After transferring the named component, checks if both source and target have a `pending` component
- If source pending is non-empty: merges into target pending, then clears source
- Skips if transferred component IS "pending" (no double-transfer)
- Skips if source pending is empty

**Tests:** 2 new tests. 458 total, all passing, clippy clean. Binary installed.
Please add any pending items that should have been transferred to tasks/software/instruction-files.md
### Re: Pending items transferred to instruction-files.md

Added 5 new pending items from the exchange discussion to `tasks/software/instruction-files.md`:

- Architecture: instruction-files depends on agent-runbooks + agent-memories specs
- agent-memories spec: committed memory format (type, scope, why, how to apply)
- agent-runbooks spec: runbook format (checklist, prerequisites, verification)
- Committed memory write capability (route lessons to right file)
- Compare/align with Cursor Rules, Windsurf Rules, .agents spec

Total: 11 pending items in instruction-files.md now. Committed.
When tasks/software/proof.md was claimed, the initial template was not committed. Why?
### Re: proof.md template not committed

The committed version (commit `75124382`) only has `agent_doc_session:` — the bare frontmatter from the old `ensure_initialized()` before the scaffold fix.

The current file has the full template (different UUID `23463bb9` — recreated after the fix), but sync's auto-scaffold didn't commit it. The issue: sync calls `ensure_initialized()` after scaffolding, which creates the snapshot and calls `ensure_git_tracked()`. But `ensure_git_tracked()` only commits if the file is untracked. Since the old bare version was already tracked (committed as `dc697b65`), git sees the new content as a modification, not an untracked file — so `ensure_git_tracked()` skips the commit.

**The fix:** After auto-scaffolding in `resolve_file`, sync should explicitly call `git::commit()` to commit the scaffolded content. The scaffold is a significant change (empty file → full template) that should be committed immediately.

This is another case where the binary needs to enforce the invariant: scaffolded files should be committed. Want me to fix it?
yes
### Re: Fixed — sync now commits after scaffolding

Added `git::commit(path)` call immediately after writing the scaffold in `resolve_file`. This handles the case where the file was previously tracked with bare frontmatter — git sees the scaffold as a modification (not untracked), so `ensure_git_tracked()` skips it. The explicit commit ensures the full template is in HEAD.

458 tests pass, clippy clean. Binary installed.

When I emptied proof.md and ran `Sync Tmux Layout`, the base template populated the file but it also was not committed. Why?
### Re: Scaffold commit failed — snapshot ordering bug

Found it in ops.log: `commit_failed file=tasks/software/proof.md exit_code=1` with `drift_warning snap_len=64 file_len=346`.

**Root cause:** `git::commit()` uses the snapshot to determine what to stage. The snapshot hadn't been updated to the scaffolded content yet — it still had the old bare frontmatter (64 bytes). The commit saw a mismatch (file=346 vs snap=64) and failed.

**Fix:** Added `snapshot::save(path, &scaffold)` BEFORE `git::commit(path)` in the scaffold block. The sequence is now:
1. Write scaffold to file
2. Save scaffold as snapshot
3. Commit (snapshot matches file — no drift)

458 tests pass, clippy clean. Binary installed. Try emptying proof.md and running "Sync Tmux Layout" again.
release
### Re: Release — agent-doc v0.31.14 (HEAD)

Released:

- **crates.io**: Published agent-doc v0.31.14
- **GitHub**: https://github.com/btakita/agent-doc/releases/tag/v0.31.14
- **Binary**: installed to ~/.cargo/bin (v0.31.14)
- **Version sync**: Cargo.toml + pyproject.toml both at 0.31.14

**v0.31.14 changes:**
- Binding invariant enforcement: claim provisions new pane when occupied
- Sync auto-scaffold: empty .md files get full template + snapshot + commit
- Transfer pending merge: pending component transferred alongside exchange
- SPEC.md §7.10 + §8.5 updated
- 458 tests, code-enforced-directives runbook
<!-- agent:boundary:ab17634d -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
All items complete. v0.31.14 released.
<!-- /agent:pending -->
