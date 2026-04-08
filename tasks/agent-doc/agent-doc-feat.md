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
### Re: Document-level hooks + session push

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
### Re: go

Done. Committed to `src/agent-doc` as `feat: document-level session_start hooks via frontmatter`.

**What was implemented:**

- `frontmatter.rs` — `hooks: HashMap<String, Vec<String>>` field on `Frontmatter`; omitted from serialization when empty
- `start.rs` — `fire_doc_hooks()` called after pane provisioning; substitutes `{{session_id}}`, `{{file}}`, `{{agent}}`, `{{model}}` before `sh -c`; best-effort (log to stderr, never block)

**Tests added (7 total):** hooks roundtrip, omitted-when-empty, absent-parses-as-empty, template var substitution, noop-for-missing, noop-for-empty, None agent/model → empty string.

**Usage:**
```yaml
hooks:
  session_start:
    - "curl -s -X POST https://api.dreamable.com/sessions -d '{\"session_id\": \"{{session_id}}\", \"file\": \"{{file}}\"}'"
```

`post_write` and `post_commit` are schema-ready but call sites in `hooks.rs` are not yet wired — that's a follow-on.

❯ do
❯ - [ ] Wire `post_write` / `post_commit` events to `fire_doc_hooks` in `hooks.rs`
### Re: post_write / post_commit wiring

Done. Second commit: `feat: wire post_write/post_commit document-level hooks`.

**Changes:**
- `hooks.rs` — `fire_doc_hooks` moved here from `start.rs` (correct home); `fire_doc_event(file, event)` added — reads frontmatter, calls `fire_doc_hooks`, best-effort (no-op on missing file or empty hooks)
- `start.rs` — calls `crate::hooks::fire_doc_hooks` directly (no local copy)
- `write.rs` — `fire_doc_event(file, "post_write")` after `fire_post_write` on IPC-consumed path
- `git.rs` — `fire_doc_event(file, "post_commit")` after `fire_post_commit` on successful commit

**Tests added (4 more in hooks.rs):** all vars substituted, noop-for-unknown-event, noop-for-nonexistent-file, noop-when-hooks-empty-frontmatter. Total 12 tests across this feature.

**All three lifecycle events now wired:**
```yaml
hooks:
  session_start:  # fires in start.rs when Claude pane launches
    - "..."
  post_write:     # fires in write.rs after IPC-consumed write
    - "..."
  post_commit:    # fires in git.rs after successful commit
    - "..."
```
❯ When I transferred a conversation from src/session-share/tasks/claudescore.md to tasks/dreamable/robert-ross.md, the `❯ ` prefix was committed for the entire transfer...which makes sense...given that the transfer is a prompt. Why was it removed in the uncommitted text?  
### Re: ❯ prefix removed in uncommitted (editor) after transfer

The normalization adds `❯ ` on the **disk write path** and sends it to the IPC plugin via `normalize_prefix_lines`. But there's a known conflict: when `normalize_prefix_lines` is non-empty AND component patches also exist, `fullContent` (with `❯ `) is intentionally NOT sent — to avoid the duplicate-on-next-cycle bug.

So the plugin receives:
1. `normalize_prefix_lines` — add `❯ ` to specified lines in the editor buffer
2. Component patches — modify the exchange component

If the exchange `patch=append` writes out content that was assembled before normalization ran (i.e., the patch body doesn't contain the `❯ `-prefixed versions), the patch overwrites the normalized lines in the editor.

**Net result:**
- **Disk / committed**: disk path runs normalization before writing → `❯ ` present → committed correctly
- **Editor / uncommitted**: plugin applies `normalize_prefix_lines` → exchange patch overwrites those lines without prefix → editor shows no `❯ `

**Why transfer hits it:** The transfer injects a large user block (all lines are "new" for normalization) while simultaneously producing an exchange component patch. Normal agent-response cycles don't trigger this because the agent's own response lines are in the agent region, not the user region — normalization ignores them.

**Fix direction:** Normalization should run *after* patches are assembled for the IPC payload so the patch body already contains `❯ `-prefixed lines, making `normalize_prefix_lines` redundant for that content. Or: re-apply normalization to the exchange component content after `apply_patches_with_overrides` inside the plugin itself.

❯ Can agent doc send messages to claude in real-time while claude is executing a task?
```
❯ ✶ Quantumizing… (2m 47s · ↓ 661 tokens · thinking)
❯   ⎿  Tip: Send messages to Claude while it works to steer Claude in real-time


  ❯ /agent-doc tasks/agent-doc/agent-doc-bugs.md
```
### Re: Real-time message injection while Claude executes (HEAD)

**Short answer:** Not natively yet, but it's buildable — the mechanism is `tmux send-keys`.

Claude Code's "send messages while it works" feature reads from the terminal's stdin. When Claude is in the thinking/executing state, keystrokes typed in that pane get queued as a real-time steering message.

**The plumbing agent-doc already has:**
- `agent-doc` knows which tmux pane owns each document (claim/pane lifecycle)
- `tmux send-keys -t <pane> "text" Enter` can inject text into that pane's stdin

**What a `realtime_steer` feature would look like:**
1. A file watcher (inotify) monitors the document for writes while the claim pane is active
2. When the user edits `&lt;!-- agent:input --&gt;` or adds a `❯ ` line during an active Claude session, the watcher fires
3. The watcher calls `tmux send-keys` to the Claude pane with the new content

**Key challenge:** Distinguishing "Claude is mid-execution and ready to receive" from "Claude is idle". Claude Code shows the streaming indicator (the `✶ Quantumizing…` line) only during execution — we'd need to check pane content or process state to know when injection is safe.

**Simpler first step:** A manual `agent-doc steer <FILE>` command that reads a designated steering block from the document, finds the claim pane, and injects via `tmux send-keys`. No watcher needed — user triggers it explicitly when they want to steer.

**Feasibility:** High. The hard parts (pane tracking, tmux IPC) are already solved. The watcher + state detection would be the new work.
<!-- agent:boundary:41e73e0b -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Fix IPC plugin: `❯ ` prefix lost in editor after transfer (normalize runs before patch body assembled)
- [ ] Bump version + release agent-doc with hooks feature
- [ ] `agent-doc steer <FILE>` — inject steering message to claim pane via tmux send-keys
- [ ] (stretch) inotify watcher for automatic real-time steering on document edit
<!-- /agent:pending -->
