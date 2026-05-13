export type SessionCommandName = 'status' | 'restart-supervisor' | 'clear' | 'interrupt-clear' | 'doctor';

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
}

const BUSY_CLEAR_REFUSAL_HEADER_REGEX =
    /session_clear refused for (.+?) because pane (\S+) is alive-busy/s;
const BUSY_CLEAR_SOURCE_REGEX = /source=([^,)]+)/s;
const BUSY_CLEAR_COMMAND_REGEX = /current_command=([^,)]+)/s;

export function buildSessionCommandArgs(
    command: SessionCommandName,
    relativePath: string,
): string[] {
    return ['session', command, relativePath];
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

export function buildBusySessionClearBlockedMessage(
    relativePath: string,
    refusal: BusySessionClearRefusal,
): string {
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
        .trim();
    rawTail = rawTail.replace(/\)\.$/, '').replace(/\)$/, '').trim();
    if (rawTail.startsWith('"') && rawTail.endsWith('"')) {
        rawTail = rawTail.slice(1, -1);
    }
    return rawTail.replace(/\\"/g, '"').replace(/\\\\/g, '\\');
}
