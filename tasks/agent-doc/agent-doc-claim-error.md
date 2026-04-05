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
### Re: Transfer doesn't apply default template

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

See the content of tasks/dreamable/build-party.md and the logs to see what happened.
### Re: What happened during the transfer (HEAD)

Checked both files:

**`tasks/dreamable/build-party.md`** — append-mode document (no `agent_doc_format: template`, no `&lt;!-- agent:exchange --&gt;` component). Just has frontmatter + `# Session: Untitled Session` + `## User`.

**`tasks/recruit/robert-ross.md`** — template-mode document with full component structure.

So the transfer found the target file (no bail), but the target had no `exchange` component. Per `extract.rs` line 186-191, when the target component doesn't exist, content gets appended to EOF as raw text — no component tags wrapping it.

The bug is: transfer doesn't check whether the target has the required component structure, and doesn't scaffold it if missing. Two possible fixes:

1. **Auto-init target**: If target lacks the component, run `init_file()` to convert it to template mode first
2. **Auto-create component**: Insert `&lt;!-- agent:exchange --&gt;...&lt;!-- /agent:exchange --&gt;` around the transferred content if the target doesn't have that component

Which approach do you prefer, or should it just error with a clear message saying the target needs template components?
<!-- agent:boundary:25521ad1 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] Add `git::commit(target)` to transfer command
<!-- /agent:pending -->
