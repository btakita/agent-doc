---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
Exchange transferred to instruction-files task. v0.31.14 pending release.
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
### Re: proof.md template not committed (HEAD)

The committed version (commit `75124382`) only has `agent_doc_session:` — the bare frontmatter from the old `ensure_initialized()` before the scaffold fix.

The current file has the full template (different UUID `23463bb9` — recreated after the fix), but sync's auto-scaffold didn't commit it. The issue: sync calls `ensure_initialized()` after scaffolding, which creates the snapshot and calls `ensure_git_tracked()`. But `ensure_git_tracked()` only commits if the file is untracked. Since the old bare version was already tracked (committed as `dc697b65`), git sees the new content as a modification, not an untracked file — so `ensure_git_tracked()` skips the commit.

**The fix:** After auto-scaffolding in `resolve_file`, sync should explicitly call `git::commit()` to commit the scaffolded content. The scaffold is a significant change (empty file → full template) that should be committed immediately.

This is another case where the binary needs to enforce the invariant: scaffolded files should be committed. Want me to fix it?
<!-- agent:boundary:fc62cda9 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Fix sync auto-scaffold: commit after scaffolding
- [ ] Bump agent-doc to v0.31.14 and release
<!-- /agent:pending -->
