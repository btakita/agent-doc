export type PopupMenuActionId =
    | 'submit'
    | 'startAgentDoc'
    | 'initSession'
    | 'claim'
    | 'claimArtifact'
    | 'fixDocument'
    | 'compactExchange'
    | 'syncLayout'
    | 'loadTmuxWindow'
    | 'status'
    | 'restartSupervisor'
    | 'restartAgent'
    | 'clear'
    | 'interruptClear'
    | 'doctor'
    | 'runWithJunie'
    | 'forceClaim'
    | 'stopAgent'
    | 'cancelTurn'
    | 'killSupervisor'
    | 'resyncFixSessions'
    | 'gcStaleSessions'
    | 'about'
    | 'dashboard'
    | 'treeStatus';

export interface PopupMenuItem {
    label: string;
    id: PopupMenuActionId;
}

export function buildPrimaryPopupMenuItems(): PopupMenuItem[] {
    return [
        { label: '[1] $(play) Run (Submit)', id: 'submit' },
        { label: '[2] $(new-file) Initialize Session', id: 'initSession' },
        { label: '[3] $(link) Claim Control Session', id: 'claim' },
        { label: '[4] $(tools) Fix Document', id: 'fixDocument' },
        { label: '[5] $(archive) Compact Exchange', id: 'compactExchange' },
        { label: '[6] $(layout) Sync Layout', id: 'syncLayout' },
        { label: '[7] $(window) Load Tmux Window', id: 'loadTmuxWindow' },
        { label: '[8] $(pulse) Show Session Status', id: 'status' },
        { label: '[9] $(clear-all) Clear Session Context', id: 'clear' },
        // After Clear so [9] stays Clear. Still the first quick-pick screen.
        { label: '[10] $(link) Claim Artifact Session', id: 'claimArtifact' },
        { label: '[11] $(debug-restart) Recycle Supervisor', id: 'restartSupervisor' },
        { label: '[12] $(debug-restart) Restart Agent', id: 'restartAgent' },
        { label: '[13] $(warning) Interrupt and Clear Session Context', id: 'interruptClear' },
        { label: '[14] $(copy) Copy Session Diagnostics', id: 'doctor' },
        { label: '[15] $(dashboard) Dashboard', id: 'dashboard' },
        { label: '[16] $(list-tree) Tree Status', id: 'treeStatus' },
        { label: '[17] $(terminal) Start Agent Doc', id: 'startAgentDoc' },
        { label: '[18] $(hubot) Run with Junie', id: 'runWithJunie' },
        { label: '[19] $(warning) Force Claim Control Session', id: 'forceClaim' },
        { label: '[20] $(debug-stop) Stop Agent', id: 'stopAgent' },
        { label: '[21] $(circle-slash) Cancel Turn', id: 'cancelTurn' },
        { label: '[22] $(trash) Kill Supervisor', id: 'killSupervisor' },
        { label: '[23] $(sync) Resync / Fix Sessions', id: 'resyncFixSessions' },
        { label: '[24] $(database) GC Stale Sessions', id: 'gcStaleSessions' },
        { label: '[25] $(info) About Agent Doc', id: 'about' },
    ];
}
