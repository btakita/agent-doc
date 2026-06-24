export type PopupMenuActionId =
    | 'submit'
    | 'claim'
    | 'compactExchange'
    | 'syncLayout'
    | 'status'
    | 'restartSupervisor'
    | 'restartAgent'
    | 'clear'
    | 'doctor'
    | 'more'
    | 'runWithJunie'
    | 'forceClaim';

export interface PopupMenuItem {
    label: string;
    id: PopupMenuActionId;
}

export function buildPrimaryPopupMenuItems(): PopupMenuItem[] {
    return [
        { label: '[1] $(play) Run (Submit)', id: 'submit' },
        { label: '[2] $(link) Claim', id: 'claim' },
        { label: '[3] $(archive) Compact Exchange', id: 'compactExchange' },
        { label: '[4] $(layout) Sync Layout', id: 'syncLayout' },
        { label: '[5] $(pulse) Show Session Status', id: 'status' },
        { label: '[6] $(debug-restart) Recycle Supervisor', id: 'restartSupervisor' },
        { label: '[7] $(debug-restart) Restart Agent', id: 'restartAgent' },
        { label: '[8] $(clear-all) Clear Session Context', id: 'clear' },
        { label: '[9] $(copy) Copy Session Diagnostics', id: 'doctor' },
        { label: '$(kebab-horizontal) More Actions', id: 'more' },
    ];
}

export function buildOverflowPopupMenuItems(): PopupMenuItem[] {
    return [
        { label: '$(hubot) Run with Junie', id: 'runWithJunie' },
        { label: '$(warning) Force Claim for Tmux Pane', id: 'forceClaim' },
    ];
}
