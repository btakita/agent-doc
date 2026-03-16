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

### Inline component attributes implemented

All 372 tests pass, clippy clean. Changes across 6 files:

**`component.rs`** — Parser extracts `key=value` attributes from opening tags. `Component` struct has `attrs: HashMap<String, String>`.

**`template.rs` + `patch.rs`** — Mode resolution: inline attr > components.toml > built-in default.

**`frontmatter.rs`** — `agent_doc_format: inline` is canonical (accepts `append` as alias).

**`claim.rs`** — Scaffolding uses `<!-- agent:exchange mode=append -->`, `<!-- agent:status mode=replace -->`.

**11 new tests** covering attr parsing, mode precedence, backward compat.

Ready to release v0.17.12?
Release

I didn't see the response to `Release`...why?

```
~/work/btakita/agent-loop main ⇡32 !6                                                                                                                                                                                                 02:12:12
❯ agent-doc --version
agent-doc 0.17.0
```
<!-- /agent:exchange -->
