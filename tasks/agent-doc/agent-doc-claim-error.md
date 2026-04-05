---
agent_doc_session: bef800bc-950e-4f9e-b861-92c249853397
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
v0.31.5 released. Commit-on-claim, auto-setup, VCS refresh, --diff-only, skill runbooks, .ai/runbooks convention.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
*Compacted. JetBrains perf: GC 3.9GB blob, VCS refresh debounce 500ms, plugin timing instrumentation v0.2.42, state diagnostics logging. `agent-doc clean --archive`. Plugin version check in make precommit. Boundary (HEAD) bug: transfer now commits target so transferred headings are in git HEAD. All genuinely new headings get (HEAD). Perf issue was ephemeral (restart fixed).*

When transferring from tasks/recruit/robert-ross to tasks/dreamable/build-party.md the default element template was not added to the file content. Why?
### Re: Transfer doesn't apply default template (HEAD)

The `transfer` command in `extract.rs` requires the target file to already exist:

```rust
if !target.exists() {
    anyhow::bail!("target file not found: {}", target.display());
}
```

It never calls `init_file()` to scaffold a new document. The default template (frontmatter + `&lt;!-- agent:exchange --&gt;` component structure) only lives in `init.rs` and is only invoked by `agent-doc init`.

So when you transferred to `tasks/dreamable/build-party.md`, either:
1. The file didn't exist and transfer failed with an error, or
2. The file existed but was a bare file without template components — transfer appended content to EOF without wrapping it in `&lt;!-- agent:exchange --&gt;` tags

The fix would be: if the target doesn't exist, auto-create it via `init_file()` before proceeding with the transfer. This would give it proper frontmatter and component structure.
<!-- agent:boundary:72b2c2a9 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] Add `git::commit(target)` to transfer command
<!-- /agent:pending -->
