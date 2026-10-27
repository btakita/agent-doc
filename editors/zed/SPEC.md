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
  the resulting complete visible state is projected from `didChange`; `didSave`
  projects the same revision as disk-persisted only after the live shadow,
  replica, and disk bytes are exact. A third-party disk value remains in Zed's
  native conflict flow and never replaces editor authority. Delivery is derived
  cumulatively from that state, with no per-update acknowledgement.
- Registration always bootstraps from controller canonical state. A divergent
  opening buffer is projected downstream with `workspace/applyEdit`; it is
  never published upstream as a whole-document baseline. Because Zed reports
  full-sync changes, the LSP reduces each operator observation to the smallest
  contiguous code-point delta before publishing it.
- Remote edit echoes are generation-fenced by comparing the visible buffer to
  the already-advanced local replica; they project visible state and are never
  rebroadcast.
- The LSP process PID is part of registration so the controller's process-exit
  watcher owns crash-safe editor authority.
- Registration also names the editor: `editor_kind` `zed`, `editor_version`
  (the `extension.toml` version, which the extension passes as
  `AGENT_DOC_ZED_EXTENSION_VERSION`), `editor_binary_version` (the agent-doc
  binary), `capabilities` (exactly the `supported`/`conditional` entries of the
  `zed` column in `editors/plugin-parity.tsv`, so none today), and
  `layout_authority` `document_only`. The controller logs each registration
  against its paired generations as `controller_crdt_replica_editor_generation`
  and returns it as `editor_generation`. It does not fold the registration into
  the liveness plane, because that would select `zed-lsp` for PID-endpoint
  delivery and `zed-lsp` has no editor endpoint.
- Zed is routed by document only. Neither the extension API nor LSP exposes
  panes, splits or the focused editor, so the controller drops any layout
  evidence on an editor route whose source is Zed
  (`controller_editor_route_document_only reason=zed_exposes_no_layout`). The
  route then ensures only the routed document's pane. It never awaits or
  borrows another editor's measured layout.
- A replaced agent-doc binary is picked up in place. `zed-lsp` watches the
  canonical path it was started from. When a different executable settles
  there (seen unchanged on two checks one second apart), the server writes a
  private `agent_doc_zed_lsp_handoff/v1` file. The file holds the unparsed
  input bytes, the next request id, and each open document's replica state,
  client id, shadow and pending `applyEdit` target. The server then runs the
  replacement once as a child to validate that exact file. Only if the child
  answers `agent_doc_zed_lsp_handoff/v1 ok` does the server `execve` into the
  replacement. The PID, stdio, controller identity and CRDT peer are all
  unchanged, so neither Zed nor the controller sees a restart. The server
  reads standard input only on its serving thread, and only between handled
  messages, so no byte is held anywhere the handoff cannot carry. A
  replacement that fails validation, a failed `execve`, or a host without
  `execve` leaves the running server serving. It is then refused once and the
  operator is asked, with `window/showMessage`, to run
  `editor: restart language server`.
- Zed extensions have no editor action or menu surface, so the shared
  `About Agent Doc` action (`editors/SPEC.md` section 6b) does not exist here.
  The language server reports its version in the LSP `initialize` response's
  `serverInfo`; `agent-doc version` reports the binary.
