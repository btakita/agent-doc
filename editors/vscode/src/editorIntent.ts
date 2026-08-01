/** Shared semantic names for messages sent to a registered editor endpoint. */
export const EditorIntent = {
    ApplyCanonical: 'apply_canonical',
    /** Apply node-keyed structural ops (strike / mark_done) WITHOUT a whole-buffer
     * canonical replace. Carries `node_patches` only (`#crdtstructops` Phase C). */
    ApplyStructuralOp: 'apply_structural_op',
    Reposition: 'reposition',
    RefreshContent: 'refresh_content',
    ObserveLazilyCurrent: 'observe_lazily_current',
    DeliverCrdtRemote: 'deliver_crdt_remote',
    RefreshVcs: 'refresh_vcs',
    ReloadLibrary: 'reload_library',
} as const;
