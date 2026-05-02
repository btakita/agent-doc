import { describe, it } from 'node:test';
import assert from 'node:assert';
import { buildTabChangeCommand } from './tabSync';

describe('buildTabChangeCommand', () => {
    it('returns sync with no autostart when the visible markdown set changes', () => {
        const planned = buildTabChangeCommand({
            activeFile: 'src/boost-client/tasks/monsterrodholders.md',
            visibleMd: [
                'tasks/agent-doc/agent-doc-bugs2.md',
                'src/boost-client/tasks/monsterrodholders.md',
            ],
            previous: {
                activeFile: 'tasks/agent-doc/agent-doc-bugs2.md',
                visibleSignature: 'tasks/agent-doc/agent-doc-bugs2.md',
            },
        });

        assert.deepStrictEqual(planned?.command, {
            kind: 'sync',
            args: [
                'sync',
                '--col',
                'src/boost-client/tasks/monsterrodholders.md,tasks/agent-doc/agent-doc-bugs2.md',
                '--focus',
                'src/boost-client/tasks/monsterrodholders.md',
                '--no-autostart',
            ],
        });
    });

    it('returns focus when only the active file changes', () => {
        const planned = buildTabChangeCommand({
            activeFile: 'src/boost-client/tasks/monsterrodholders.md',
            visibleMd: [
                'tasks/agent-doc/agent-doc-bugs2.md',
                'src/boost-client/tasks/monsterrodholders.md',
            ],
            previous: {
                activeFile: 'tasks/agent-doc/agent-doc-bugs2.md',
                visibleSignature: 'src/boost-client/tasks/monsterrodholders.md\u0000tasks/agent-doc/agent-doc-bugs2.md',
            },
        });

        assert.deepStrictEqual(planned?.command, {
            kind: 'focus',
            args: ['focus', 'src/boost-client/tasks/monsterrodholders.md'],
        });
    });

    it('returns null when the selection state is unchanged', () => {
        const result = buildTabChangeCommand({
            activeFile: 'src/boost-client/tasks/monsterrodholders.md',
            visibleMd: [
                'tasks/agent-doc/agent-doc-bugs2.md',
                'src/boost-client/tasks/monsterrodholders.md',
            ],
            previous: {
                activeFile: 'src/boost-client/tasks/monsterrodholders.md',
                visibleSignature: 'src/boost-client/tasks/monsterrodholders.md\u0000tasks/agent-doc/agent-doc-bugs2.md',
            },
        });

        assert.strictEqual(result, null);
    });
});
