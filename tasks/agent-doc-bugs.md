---
agent_doc_session: 92fba2ac-9d1f-4a7a-8764-821a8a08ecd0
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

<!-- agent:exchange -->

### Re: Inline component attributes

Love this direction. Inline attributes on the component tag is much cleaner than a separate config file:

```markdown
<!-- agent:exchange mode=append -->
Content here...
<!-- /agent:exchange -->

<!-- agent:status mode=replace -->
- [x] Phase 1
<!-- /agent:status -->
```

**Revised settings model:**

| Level | Setting | Values |
|-------|---------|--------|
| **Document** | `agent_doc_format` | `template` \| `inline` |
| **Document** | `agent_doc_write` | `crdt` \| `merge` |
| **Component** | `mode` (inline attr) | `append` \| `replace` \| `prepend` |

- `components.toml` becomes optional (project-wide defaults only)
- Inline `mode=` on the tag takes precedence over `components.toml`
- Default: `mode=append` for `exchange` and `findings`, `mode=replace` for `status`

**Implementation:**
1. Update `component.rs` parser to extract attributes from opening tags
2. `patch.rs` reads the inline mode when applying patches
3. `components.toml` remains as fallback defaults
4. Rename `agent_doc_format: append` → `inline` with backward compat alias
5. Migrate existing `append` values

**For the tag syntax:** `<!-- agent:name mode=append -->` is cleanest — HTML comment attributes, no quotes needed for simple values.

Shall I implement this? It's a parser change in `component.rs` + `patch.rs`.

yes
<!-- /agent:exchange -->
