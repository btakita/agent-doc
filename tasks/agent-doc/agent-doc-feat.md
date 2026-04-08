---
agent_doc_session: 09760345-ef9d-426a-9416-250563e42548
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
❯ Can we have document level hooks that push a session to a service?
❯ Perhaps when the claude session starts, we can run plugin instructions within a frontmatter list?

❯  See tasks/dreamable/robert-ross.md
### Re: Document-level hooks + session push (HEAD)

Yes — feasible and fits naturally into the existing architecture.

**Existing foundation:** `hooks.rs` already has project-level hooks (`post_write`, `post_commit`, etc.) via `agent-kit`. The missing piece is *document-level* hooks declared in frontmatter.

**Proposed frontmatter format:**

```yaml
hooks:
  session_start:
    - "curl -s -X POST https://api.dreamable.com/sessions \
        -H 'Content-Type: application/json' \
        -d '{\"session_id\": \"{{session_id}}\", \"file\": \"{{file}}\"}'"
  post_write:
    - "some-notify-script {{file}}"
```

Template vars: `{{session_id}}`, `{{file}}`, `{{agent}}`, `{{model}}`

**Lifecycle points to support:**
- `session_start` — fires in `start.rs` when Claude is launched in the pane
- `post_write` — fires after each write cycle (already has `fire_post_write`)
- `post_commit` — fires after git commit

**Implementation path:**
1. Parse `hooks` key in `frontmatter.rs` → `HashMap<String, Vec<String>>`
2. Shell-expand template vars, execute with `std::process::Command` in `start.rs` for `session_start`
3. `hooks.rs` fire functions call the document hooks after project-level ones
4. Best-effort: hook failures log to stderr, never block

**Connection to robert-ross.md:** `session_start` fires when `agent-doc start` provisions the pane — lets you register the session with an external service automatically.

**Re: plugins list in frontmatter:**
`hooks` with shell commands covers this without a separate plugin system. Named/reusable plugins (`~/.config/agent-doc/plugins.toml` lookup) can layer on top later if needed.

**Scope to build now:** `session_start` hook execution in `start.rs`.
❯ go
<!-- agent:boundary:d334a9ef:agent-doc-feat -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Implement `session_start` hook in `start.rs`
  - Parse `hooks.session_start` from frontmatter
  - Shell-expand template vars (`{{session_id}}`, `{{file}}`, `{{agent}}`, `{{model}}`)
  - Execute each command with `std::process::Command` after pane is provisioned
  - Best-effort: log failures to stderr, never block
<!-- /agent:pending -->
