---
agent_doc_session: 92fba2ac-9d1f-4a7a-8764-821a8a08ecd0
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

<!-- agent:status mode=replace -->
- [x] Phase 1
<!-- /agent:status -->

<!-- agent:exchange mode=append -->

### Re: Test append with 0.17.12

Good — you're now on 0.17.12. This response should **append** below the previous one (not replace it). If you can see both "Re: Append mode not working + IPC bug found" AND this "Re: Test append" section in the exchange component, append mode is working.

Using `--force-disk` to bypass the IPC bug for now.

### Re: Append mode fix

Found and fixed the root cause: `run_stream()` in `write.rs` had a **hardcoded** `mode_overrides.insert("exchange", "replace")` that forced replace mode regardless of inline attributes or `components.toml`. Removed the override — mode resolution now uses the proper chain: inline attr > components.toml > built-in default.

This response should appear BELOW the previous one. Both "Re: Append mode not working + IPC bug found" section from before (which got lost to the replace override) is gone, but this and future responses will accumulate.

Binary rebuilt. Ready to release v0.17.13 with the fix?

release

### v0.17.13 Released

Exchange append mode fix shipped:
- crates.io: https://crates.io/crates/agent-doc/0.17.13
- GitHub: https://github.com/btakita/agent-doc/releases/tag/v0.17.13

Fixed the hardcoded `mode_overrides.insert("exchange", "replace")` in `run_stream()` that was forcing replace mode. Append mode now works correctly — responses accumulate in the exchange component.
audit docs

### Re: Audit docs

4 instruction files are stale relative to `src/write.rs` (which we modified for the append mode fix):

```
SKILL.md    ✗ Older than src/write.rs
CLAUDE.md   ✗ Older than src/write.rs
README.md   ✗ Older than src/write.rs
SPEC.md     ✗ Older than src/write.rs
```

Line budget: 375/1000 (within budget).

These need updates to document:
- Inline component attributes (`mode=append` on tags)
- `agent_doc_format: inline` as canonical value
- `--force-disk` flag
- `agent-doc history` command
- IPC-priority behavior (exit 75 on timeout)
- Exchange append mode (no more hardcoded replace override)

Want me to update all 4 docs now?

Yes

### Docs updated

All 4 instruction files updated and audit passes:

- **CLAUDE.md** — inline attrs, `history.rs` in layout, IPC `--force-disk` + exit 75, flush behavior updated
- **README.md** — history subcommand, `--force-disk`, inline attr examples, `inline` format name
- **SPEC.md** — inline attrs section, mode precedence, history spec, IPC behavior, default components
- **SKILL.md** — history subcommand, inline attr syntax, `--force-disk`, renamed to "inline mode"

Combined line budget: 374/1000. Ready to commit + release v0.17.14 with the doc updates?

The response was not appended in this tasks/kevin-volk.md. Why?

Are we relying on 
<!-- /agent:exchange -->
