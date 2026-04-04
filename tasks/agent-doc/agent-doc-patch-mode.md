---
agent_doc_session:
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

# agent-doc --patch-mode

<!-- agent:status -->
**Status:** Proposed | **Component:** `agent-doc write` | **Priority:** Medium
<!-- /agent:status -->

## Architecture

<!-- agent:architecture -->
**Feature:** Add `--patch-mode replace|append|prepend` flag to `agent-doc write` that overrides the component's `patch=` attribute for a single write operation.

**Motivation:** When the skill needs to compact/restructure an exchange component (which defaults to `patch=append`), the only current option is to use Claude's Edit tool to directly modify the file. This bypasses CRDT merge, snapshot management, and introduces Read-before-Edit race conditions.

**Proposed CLI:**
```bash
# Normal response (uses component default)
agent-doc write <FILE> --stream --origin skill

# Compact (override to replace for this write only)
agent-doc write <FILE> --stream --origin skill --patch-mode replace
```

**Behavior:**
- `--patch-mode` overrides the `patch=` attribute on the target component for that single write
- Valid values: `replace`, `append`, `prepend`
- Component definition stays as the default for future writes
- CRDT merge still applies (user edits during write are preserved)
- Snapshot updated atomically as usual

**Implementation scope:** `src/agent-doc/src/write.rs` — patch application logic already handles replace/append/prepend modes. This flag just overrides which mode is selected per-component.
<!-- /agent:architecture -->

## Exchange

<!-- agent:exchange patch=append -->

<!-- /agent:exchange -->

## Pending

<!-- agent:pending -->
- [ ] Implement `--patch-mode` CLI flag in `src/agent-doc/src/write.rs`
- [ ] Update SKILL.md to document `--patch-mode` usage for compaction
- [ ] Add test case for patch-mode override
<!-- /agent:pending -->

## Todo (HEAD)

<!-- agent:todo -->
- [ ] Consider per-patch-block mode override syntax (e.g., `<!-- patch:exchange mode=replace -->`)
- [ ] Update compact exchange instructions in SKILL.md
<!-- /agent:todo -->
