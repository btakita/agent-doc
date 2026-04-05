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
### Re: Pending transfer implemented (HEAD)

Done. `agent-doc transfer` now automatically transfers the pending component alongside the named component.

**What changed in `extract.rs`:**
- After transferring the named component, checks if both source and target have a `pending` component
- If source pending is non-empty: merges into target pending, then clears source
- Skips if transferred component IS "pending" (no double-transfer)
- Skips if source pending is empty

**Tests:** 2 new tests. 458 total, all passing, clippy clean. Binary installed.
<!-- agent:boundary:7657ea9e -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] Transfer also moves pending — DONE
- [ ] Bump agent-doc to v0.31.14 and release
<!-- /agent:pending -->
