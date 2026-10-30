export function buildStartAgentDocArgs(relativePath: string, clientId: string): string[] {
    return ['tmux', 'start', relativePath, '--client-id', clientId];
}

export function buildIdeTerminalEnsureArgs(relativePath: string, clientId: string): string[] {
    return ['tmux', 'ensure', relativePath, '--json', '--ide-terminal', '--client-id', clientId];
}
