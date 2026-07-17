/** Shared semantic names for messages sent to a registered editor endpoint. */
export const EditorIntent = {
    ApplyCanonical: 'apply_canonical',
    Reposition: 'reposition',
    SaveDocument: 'save_document',
    RefreshContent: 'refresh_content',
    ObserveLazilyCurrent: 'observe_lazily_current',
    DeliverCrdtRemote: 'deliver_crdt_remote',
    RefreshVcs: 'refresh_vcs',
    ReloadLibrary: 'reload_library',
} as const;
