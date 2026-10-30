import assert from 'node:assert/strict';
import test from 'node:test';

import { buildIdeTerminalEnsureArgs, buildStartAgentDocArgs } from './startAgentDoc.js';

test('start command names the document and VS Code navigation client', () => {
    assert.deepEqual(buildStartAgentDocArgs('tasks/plan.md', 'vscode-pid:41'), [
        'tmux',
        'start',
        'tasks/plan.md',
        '--client-id',
        'vscode-pid:41',
    ]);
});

test('terminal ensure follows start in the same navigation lane', () => {
    assert.deepEqual(buildIdeTerminalEnsureArgs('tasks/plan.md', 'vscode-pid:41'), [
        'tmux',
        'ensure',
        'tasks/plan.md',
        '--json',
        '--ide-terminal',
        '--client-id',
        'vscode-pid:41',
    ]);
});
