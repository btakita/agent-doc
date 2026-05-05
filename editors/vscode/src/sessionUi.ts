export type SessionCommandName = 'status' | 'restart-supervisor' | 'clear' | 'doctor';

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

function normalizeOutputBody(output: string): string {
    const trimmed = output.trim();
    return trimmed || '(no output)';
}
