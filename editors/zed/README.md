# agent-doc for Zed

This extension adds agent-doc realtime synchronization to Zed's existing
Markdown language. It does not define a second Markdown grammar.

The language server is launched for Markdown buffers, but remains a strict
no-op unless the current buffer has valid frontmatter with a non-empty
`agent_doc_session` field. Adding the field attaches the buffer; removing it
detaches the buffer. Normal Markdown files retain Zed's ordinary Markdown
features and receive no agent-doc edits or commands.

## Development install

1. Install the current `agent-doc` binary so it is visible in Zed's worktree
   `PATH`.
2. In Zed, choose **Extensions → Install Dev Extension**.
3. Select this `editors/zed` directory.

The extension starts `agent-doc zed-lsp`. You can override the executable or
arguments with Zed's `lsp.agent-doc.binary` settings.
