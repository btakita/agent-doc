export const IDE_HOSTED_TMUX_CAPABILITY = 'ide_hosted_tmux_v1';

export interface TmuxEnsureReceipt {
    sessionName: string;
    paneId: string;
    attachCommand: string;
    created: boolean;
    attached: boolean;
    terminalHost: string;
    terminalHostReason: string;
    autoStartTmux: boolean;
}

export enum IdeTerminalAttachDecision {
    NoopExternalAttached = 'noop_external_attached',
    NoopConfiguredHost = 'noop_configured_host',
    FocusExisting = 'focus_existing',
    AttachExisting = 'attach_existing',
    CreateAndAttach = 'create_and_attach',
}

export function parseTmuxEnsureReceipt(raw: string): TmuxEnsureReceipt {
    const value = JSON.parse(raw) as Record<string, unknown>;
    if (
        typeof value.session_name !== 'string'
        || typeof value.pane_id !== 'string'
        || typeof value.attach_command !== 'string'
        || typeof value.created !== 'boolean'
        || typeof value.attached !== 'boolean'
        || typeof value.terminal_host !== 'string'
        || typeof value.terminal_host_reason !== 'string'
        || typeof value.auto_start_tmux !== 'boolean'
    ) {
        throw new Error('tmux ensure returned an invalid receipt');
    }
    return {
        sessionName: value.session_name,
        paneId: value.pane_id,
        attachCommand: value.attach_command,
        created: value.created,
        attached: value.attached,
        terminalHost: value.terminal_host,
        terminalHostReason: value.terminal_host_reason,
        autoStartTmux: value.auto_start_tmux,
    };
}

export function decideIdeTerminalAttach(
terminalHost: string,
sessionAttached: boolean,
    existingTerminalAlive: boolean,
): IdeTerminalAttachDecision {
    if (sessionAttached && existingTerminalAlive) {
        return IdeTerminalAttachDecision.FocusExisting;
    }
    if (sessionAttached) {
        return IdeTerminalAttachDecision.NoopExternalAttached;
    }
    if (terminalHost !== 'ide') {
        return IdeTerminalAttachDecision.NoopConfiguredHost;
    }
    if (existingTerminalAlive) {
        return IdeTerminalAttachDecision.AttachExisting;
    }
    return IdeTerminalAttachDecision.CreateAndAttach;
}
