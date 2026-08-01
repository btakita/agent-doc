# Zed extension contract

- Register one supplemental language server for Zed's existing `Markdown`
  language. Never claim `.md` with a competing language definition.
- Enter agent-doc mode only when the live buffer parses as agent-doc
  frontmatter and has a non-empty `agent_doc_session`.
- Re-evaluate the mode gate on every full-buffer LSP change. Adding the marker
  attaches; removing or invalidating it immediately deregisters the replica.
- A non-agent-doc Markdown document is a strict no-op: no controller request,
  no workspace edit, no diagnostic, and no replacement of normal Markdown
  behavior.
- In agent-doc mode, local changes publish CRDT deltas to the project
  controller. Controller deltas are applied through `workspace/applyEdit` and
  acknowledged only after the resulting visible buffer hash is observed in
  `didChange`.
- Registration always bootstraps from controller canonical state. A divergent
  opening buffer is projected downstream with `workspace/applyEdit`; it is
  never published upstream as a whole-document baseline. Because Zed reports
  full-sync changes, the LSP reduces each operator observation to the smallest
  contiguous code-point delta before publishing it.
- Remote edit echoes are generation-fenced by comparing the visible buffer to
  the already-advanced local replica; they are acknowledged, never rebroadcast.
- The LSP process PID is part of registration so the controller's process-exit
  watcher owns crash-safe editor authority.
