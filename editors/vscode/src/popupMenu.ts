export type PopupMenuActionId =
    | 'submit'
    | 'claim'
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
    | 'more'
    | 'runWithJunie'
    | 'forceClaim'
    | 'stopAgent'
    | 'cancelTurn'
    | 'killSupervisor'
    | 'resyncFixSessions'
    | 'gcStaleSessions';

export interface PopupMenuItem {
    label: string;
    id: PopupMenuActionId;
}

export function buildPrimaryPopupMenuItems(): PopupMenuItem[] {
    return [
        { label: '[1] $(play) Run (Submit)', id: 'submit' },
        { label: '[2] $(link) Claim', id: 'claim' },
        { label: '[3] $(tools) Fix Document', id: 'fixDocument' },
        { label: '[4] $(archive) Compact Exchange', id: 'compactExchange' },
        { label: '[5] $(layout) Sync Layout', id: 'syncLayout' },
        { label: '[6] $(window) Load Tmux Window', id: 'loadTmuxWindow' },
        { label: '[7] $(pulse) Show Session Status', id: 'status' },
        { label: '[8] $(debug-restart) Recycle Supervisor', id: 'restartSupervisor' },
        { label: '[9] $(debug-restart) Restart Agent', id: 'restartAgent' },
        { label: '[10] $(clear-all) Clear Session Context', id: 'clear' },
        { label: '[11] $(warning) Interrupt and Clear Session Context', id: 'interruptClear' },
        { label: '[12] $(copy) Copy Session Diagnostics', id: 'doctor' },
        { label: '$(kebab-horizontal) More Actions', id: 'more' },
    ];
}

export function buildOverflowPopupMenuItems(): PopupMenuItem[] {
    return [
        { label: '$(hubot) Run with Junie', id: 'runWithJunie' },
        { label: '$(warning) Force Claim for Tmux Pane', id: 'forceClaim' },
        { label: '$(debug-stop) Stop Agent', id: 'stopAgent' },
        { label: '$(circle-slash) Cancel Turn', id: 'cancelTurn' },
        { label: '$(trash) Kill Supervisor', id: 'killSupervisor' },
        { label: '$(sync) Resync / Fix Sessions', id: 'resyncFixSessions' },
        { label: '$(database) GC Stale Sessions', id: 'gcStaleSessions' },
    ];
}
