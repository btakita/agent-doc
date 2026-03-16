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
<!-- /agent:exchange -->
