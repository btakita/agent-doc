export type SessionCommandName = 'status' | 'restart-supervisor' | 'stop-agent' | 'cancel-turn' | 'clear' | 'interrupt-clear' | 'doctor';

export interface OutputPresentation {
    title: string;
    body: string;
}

export interface SessionStatusPresentation extends OutputPresentation {
    hint: string;
}

export interface RouteFailurePresentation extends OutputPresentation {
    toast: string;
}

export interface BusySessionClearRefusal {
    file: string;
    pane: string;
    source: string;
    currentCommand: string;
    tail: string;
    protectedReason?: string;
}

export interface BusySessionRestartRefusal {
    file: string;
    pane: string;
    source: string;
    currentCommand: string;
    tail: string;
}

export interface StartingSessionRestartRefusal {
    file: string;
    reason: string;
}

const BUSY_CLEAR_REFUSAL_HEADER_REGEX =
    /session_clear refused for (.+?) because pane (\S+) is alive-busy/s;
const PROTECTED_CLEAR_REFUSAL_HEADER_REGEX =
    /session_clear refused for (.+?) because pane (\S+) contains protected prompt input/s;
const BUSY_RESTART_REFUSAL_HEADER_REGEX =
    /session_restart refused for (.+?) because pane (\S+) is alive-busy/s;
const STARTING_RESTART_REFUSAL_REGEX =
    /session_restart refused for (.+?) because the authoritative actor is still starting and (.+?)\. Wait for a dispatch-ready prompt/s;
const BUSY_CLEAR_SOURCE_REGEX = /source=([^,)]+)/s;
const BUSY_CLEAR_COMMAND_REGEX = /current_command=([^,)]+)/s;
const PROTECTED_CLEAR_REASON_REGEX = /reason=([^,)]+)/s;

export function buildSessionCommandArgs(
    command: SessionCommandName,
    relativePath: string,
): string[] {
    return ['session', command, relativePath];
}

export function buildForcedRestartSupervisorCommandArgs(relativePath: string): string[] {
    return ['session', 'restart-supervisor', '--force', relativePath];
}

export function buildSessionStatusPresentation(
    relativePath: string,
    output: string,
): SessionStatusPresentation {
    return {
        title: `Session status: ${relativePath}`,
        body: normalizeOutputBody(output),
        hint: `Session status: ${relativePath}`,
    };
}

export function buildSessionSuccessHint(
    command: Exclude<SessionCommandName, 'status'>,
    relativePath: string,
    output: string,
): string {
    const trimmed = output.trim();
    if (trimmed) return trimmed;
    switch (command) {
        case 'restart-supervisor':
            return `Restart requested for supervisor handling ${relativePath}`;
        case 'stop-agent':
            return `Stopped agent for ${relativePath} (supervisor still running)`;
        case 'cancel-turn':
            return `Cancelled turn for ${relativePath} (no-op if the agent was idle)`;
        case 'clear':
            return `Cleared session context for ${relativePath}`;
        case 'interrupt-clear':
            return `Interrupted and cleared session context for ${relativePath}`;
        case 'doctor':
            return `Copied session diagnostics for ${relativePath}`;
    }
}

export function buildRouteFailurePresentation(
    relativePath: string,
    output: string,
): RouteFailurePresentation {
    const trimmed = output.trim();
    const firstLine = trimmed.split('\n')[0]?.trim() || 'route failed';
    return {
        title: `Route failure: ${relativePath}`,
        body: normalizeOutputBody(trimmed),
        toast: `route failed: ${firstLine}`,
    };
}

export function sessionStatusShowsIdleDirectPane(output: string): boolean {
    return output.split(/\r?\n/).some((line) =>
        line.startsWith('live_pane:') &&
        line.includes('state=alive-idle') &&
        line.includes('prompt_ready=true'),
    );
}

export function parseBusySessionClearRefusal(output: string): BusySessionClearRefusal | undefined {
    const protectedMatch = PROTECTED_CLEAR_REFUSAL_HEADER_REGEX.exec(output);
    if (protectedMatch) {
        const detail = output.slice((protectedMatch.index ?? 0) + protectedMatch[0].length);
        const source = BUSY_CLEAR_SOURCE_REGEX.exec(detail)?.[1] || 'unknown';
        const currentCommand = BUSY_CLEAR_COMMAND_REGEX.exec(detail)?.[1] || 'unknown';
        const protectedReason = PROTECTED_CLEAR_REASON_REGEX.exec(detail)?.[1] || 'protected prompt input';
        return {
            file: protectedMatch[1],
            pane: protectedMatch[2],
            source,
            currentCommand,
            tail: extractBusyClearTail(detail),
            protectedReason,
        };
    }
    const match = BUSY_CLEAR_REFUSAL_HEADER_REGEX.exec(output);
    if (!match) return undefined;

    const detail = output.slice((match.index ?? 0) + match[0].length);
    const source = BUSY_CLEAR_SOURCE_REGEX.exec(detail)?.[1] || 'unknown';
    const currentCommand = BUSY_CLEAR_COMMAND_REGEX.exec(detail)?.[1] || 'unknown';
    return {
        file: match[1],
        pane: match[2],
        source,
        currentCommand,
        tail: extractBusyClearTail(detail),
    };
}

export function parseBusySessionRestartRefusal(output: string): BusySessionRestartRefusal | undefined {
    const match = BUSY_RESTART_REFUSAL_HEADER_REGEX.exec(output);
    if (!match) return undefined;

    const detail = output.slice((match.index ?? 0) + match[0].length);
    const source = BUSY_CLEAR_SOURCE_REGEX.exec(detail)?.[1] || 'unknown';
    const currentCommand = BUSY_CLEAR_COMMAND_REGEX.exec(detail)?.[1] || 'unknown';
    return {
        file: match[1],
        pane: match[2],
        source,
        currentCommand,
        tail: extractBusyClearTail(detail),
    };
}

export function parseStartingSessionRestartRefusal(output: string): StartingSessionRestartRefusal | undefined {
    const match = STARTING_RESTART_REFUSAL_REGEX.exec(output);
    if (!match) return undefined;
    return {
        file: match[1],
        reason: match[2].trim(),
    };
}

export function buildBusySessionClearBlockedMessage(
    relativePath: string,
    refusal: BusySessionClearRefusal,
): string {
    if (refusal.protectedReason) {
        const tail = refusal.tail && refusal.tail !== 'unknown'
            ? `\nLatest pane output: ${refusal.tail}`
            : '';
        return [
            `Clear Session Context is blocked for ${relativePath}.`,
            `Pane ${refusal.pane} contains protected prompt input (${refusal.protectedReason}).`,
            'Use Interrupt and clear to discard the prompt input, or Show status to inspect the session.',
        ].join(' ') + tail;
    }
    const command = refusal.currentCommand && refusal.currentCommand !== 'unknown'
        ? ` (${refusal.currentCommand})`
        : '';
    const tail = refusal.tail && refusal.tail !== 'unknown'
        ? `\nLatest pane output: ${refusal.tail}`
        : '';
    return [
        `Session is still running for ${relativePath}.`,
        `Pane ${refusal.pane} is busy${command}.`,
        'Wait for the turn to finish, then retry Clear Session Context.',
        'Use Refresh and retry if the pane has returned to an idle prompt, or Interrupt and clear to discard the running turn.',
    ].join(' ') + tail;
}

export function buildBusySessionRestartBlockedMessage(
    relativePath: string,
    refusal: BusySessionRestartRefusal,
): string {
    const command = refusal.currentCommand && refusal.currentCommand !== 'unknown'
        ? ` (${refusal.currentCommand})`
        : '';
    const tail = refusal.tail && refusal.tail !== 'unknown'
        ? `\nLatest pane output: ${refusal.tail}`
        : '';
    return [
        `Restart Supervisor is blocked for ${relativePath}.`,
        `Pane ${refusal.pane} is busy${command}.`,
        'Use Interrupt and restart to stop the running turn and restart the supervisor, or Show status to inspect the session.',
    ].join(' ') + tail;
}

export function buildStartingSessionRestartBlockedMessage(
    relativePath: string,
    refusal: StartingSessionRestartRefusal,
): string {
    const reason = refusal.reason ? ` and ${refusal.reason}` : '';
    return [
        `Restart Supervisor is blocked for ${relativePath}.`,
        `The authoritative actor is still starting${reason}.`,
        'Use Interrupt and restart to stop the current supervisor generation and restart anyway, or Show status to inspect the session.',
    ].join(' ');
}

function normalizeOutputBody(output: string): string {
    const trimmed = output.trim();
    return trimmed || '(no output)';
}

function extractBusyClearTail(detail: string): string {
    const marker = 'tail=';
    const start = detail.indexOf(marker);
    if (start < 0) return '';
    let rawTail = detail.slice(start + marker.length)
        .split('). Run `agent-doc session status')[0]
        .split('). Run agent-doc session status')[0]
        .split('). Clear the prompt input manually')[0]
        .trim();
    rawTail = rawTail.replace(/\)\.$/, '').replace(/\)$/, '').trim();
    if (rawTail.startsWith('"') && rawTail.endsWith('"')) {
        rawTail = rawTail.slice(1, -1);
    }
    return rawTail.replace(/\\"/g, '"').replace(/\\\\/g, '\\');
}
