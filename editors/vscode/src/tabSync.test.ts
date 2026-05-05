import { describe, it } from 'node:test';
import assert from 'node:assert';
import {
    buildSyncCommandArgs,
    buildTabChangeCommand,
    shouldReplayQueuedTabChange,
    visibleSignatureFromColumns,
} from './tabSync';

describe('buildTabChangeCommand', () => {
    it('returns sync with no autostart when the visible markdown set changes', () => {
        const planned = buildTabChangeCommand({
            activeFile: 'src/boost-client/tasks/monsterrodholders.md',
            visibleMd: [
                'tasks/agent-doc/agent-doc-bugs2.md',
                'src/boost-client/tasks/monsterrodholders.md',
            ],
            visibleColumns: [
                ['tasks/agent-doc/agent-doc-bugs2.md'],
                ['src/boost-client/tasks/monsterrodholders.md'],
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
                'tasks/agent-doc/agent-doc-bugs2.md',
                '--col',
                'src/boost-client/tasks/monsterrodholders.md',
                '--focus',
                'src/boost-client/tasks/monsterrodholders.md',
                '--no-autostart',
            ],
        });
    });

    it('keeps split layouts on sync when only the active file changes', () => {
        const planned = buildTabChangeCommand({
            activeFile: 'src/boost-client/tasks/monsterrodholders.md',
            visibleMd: [
                'tasks/agent-doc/agent-doc-bugs2.md',
                'src/boost-client/tasks/monsterrodholders.md',
            ],
            visibleColumns: [
                ['tasks/agent-doc/agent-doc-bugs2.md'],
                ['src/boost-client/tasks/monsterrodholders.md'],
            ],
            previous: {
                activeFile: 'tasks/agent-doc/agent-doc-bugs2.md',
                visibleSignature: visibleSignatureFromColumns([
                    ['tasks/agent-doc/agent-doc-bugs2.md'],
                    ['src/boost-client/tasks/monsterrodholders.md'],
                ]),
            },
        });

        assert.deepStrictEqual(planned?.command, {
            kind: 'sync',
            args: [
                'sync',
                '--col',
                'tasks/agent-doc/agent-doc-bugs2.md',
                '--col',
                'src/boost-client/tasks/monsterrodholders.md',
                '--focus',
                'src/boost-client/tasks/monsterrodholders.md',
                '--no-autostart',
            ],
        });
    });

    it('returns focus when a single visible markdown file stays selected', () => {
        const planned = buildTabChangeCommand({
            activeFile: 'src/boost-client/tasks/monsterrodholders.md',
            visibleMd: ['src/boost-client/tasks/monsterrodholders.md'],
            previous: {
                activeFile: 'tasks/agent-doc/agent-doc-bugs2.md',
                visibleSignature: visibleSignatureFromColumns([
                    ['src/boost-client/tasks/monsterrodholders.md'],
                ]),
            },
        });

        assert.deepStrictEqual(planned?.command, {
            kind: 'focus',
            args: ['focus', 'src/boost-client/tasks/monsterrodholders.md'],
        });
    });

    it('keeps split layouts on sync even when only one markdown file is visible', () => {
        const planned = buildTabChangeCommand({
            activeFile: 'src/session-share/tasks/claudescore-3.md',
            visibleMd: ['src/session-share/tasks/claudescore-3.md'],
            visibleColumns: [
                [],
                ['src/session-share/tasks/claudescore-3.md'],
            ],
            previous: {
                activeFile: 'tasks/agent-doc/agent-doc-bugs2.md',
                visibleSignature: visibleSignatureFromColumns([
                    [],
                    ['src/session-share/tasks/claudescore-3.md'],
                ]),
            },
        });

        assert.deepStrictEqual(planned?.command, {
            kind: 'sync',
            args: [
                'sync',
                '--col',
                '',
                '--col',
                'src/session-share/tasks/claudescore-3.md',
                '--focus',
                'src/session-share/tasks/claudescore-3.md',
                '--no-autostart',
            ],
        });
    });

    it('returns null when the selection state is unchanged', () => {
        const result = buildTabChangeCommand({
            activeFile: 'src/boost-client/tasks/monsterrodholders.md',
            visibleMd: [
                'tasks/agent-doc/agent-doc-bugs2.md',
                'src/boost-client/tasks/monsterrodholders.md',
            ],
            visibleColumns: [
                ['tasks/agent-doc/agent-doc-bugs2.md'],
                ['src/boost-client/tasks/monsterrodholders.md'],
            ],
            previous: {
                activeFile: 'src/boost-client/tasks/monsterrodholders.md',
                visibleSignature: visibleSignatureFromColumns([
                    ['tasks/agent-doc/agent-doc-bugs2.md'],
                    ['src/boost-client/tasks/monsterrodholders.md'],
                ]),
            },
        });

        assert.strictEqual(result, null);
    });

    it('keeps opposite-pane selections on sync when the split is unchanged', () => {
        const planned = buildTabChangeCommand({
            activeFile: 'tasks/agent-doc/agent-doc-bugs2.md',
            visibleMd: [
                'tasks/agent-doc/agent-doc-bugs2.md',
                'src/boost-client/tasks/monsterrodholders.md',
            ],
            visibleColumns: [
                ['tasks/agent-doc/agent-doc-bugs2.md'],
                ['src/boost-client/tasks/monsterrodholders.md'],
            ],
            previous: {
                activeFile: 'src/boost-client/tasks/monsterrodholders.md',
                visibleSignature: visibleSignatureFromColumns([
                    ['tasks/agent-doc/agent-doc-bugs2.md'],
                    ['src/boost-client/tasks/monsterrodholders.md'],
                ]),
            },
        });

        assert.deepStrictEqual(planned?.command.kind, 'sync');
    });

    it('keeps visible-set changes on sync', () => {
        const planned = buildTabChangeCommand({
            activeFile: 'tasks/agent-doc/agent-doc-bugs2.md',
            visibleMd: [
                'tasks/agent-doc/agent-doc-bugs2.md',
                'src/boost-client/tasks/monsterrodholders.md',
            ],
            visibleColumns: [
                ['tasks/agent-doc/agent-doc-bugs2.md'],
                ['src/boost-client/tasks/monsterrodholders.md'],
            ],
            previous: {
                activeFile: 'src/boost-client/tasks/monsterrodholders.md',
                visibleSignature: visibleSignatureFromColumns([
                    ['tasks/agent-doc/agent-doc-bugs2.md'],
                ]),
            },
        });

        assert.deepStrictEqual(planned?.command.kind, 'sync');
    });

    it('keeps split placeholders in sync commands so non-markdown side panes do not collapse columns', () => {
        assert.deepStrictEqual(
            buildSyncCommandArgs(
                [
                    [],
                    ['src/session-share/tasks/claudescore-3.md'],
                ],
                'src/session-share/tasks/claudescore-3.md',
            ),
            [
                'sync',
                '--col',
                '',
                '--col',
                'src/session-share/tasks/claudescore-3.md',
                '--focus',
                'src/session-share/tasks/claudescore-3.md',
                '--no-autostart',
            ],
        );
    });

    it('replays the latest queued tab change after a running sync finishes', () => {
        assert.strictEqual(shouldReplayQueuedTabChange(3, 4), true);
        assert.strictEqual(shouldReplayQueuedTabChange(4, 4), false);
    });
});
