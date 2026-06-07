import { describe, it } from 'node:test';
import assert from 'node:assert';
import {
    analyzeTabSyncCommandResult,
    buildImmediateFocusCommandArgs,
    buildSyncCommandArgs,
    buildTabChangeCommand,
    shouldReplayQueuedTabChange,
    shouldScheduleDeferredTabSyncRetry,
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
                '--exact-visible',
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
                '--exact-visible',
                '--no-autostart',
            ],
        });
    });

    it('uses passive sync instead of focus when a single visible markdown file stays selected', () => {
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
            kind: 'sync',
            args: [
                'sync',
                '--col',
                'src/boost-client/tasks/monsterrodholders.md',
                '--focus',
                'src/boost-client/tasks/monsterrodholders.md',
                '--exact-visible',
                '--no-autostart',
            ],
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
                '--exact-visible',
                '--no-autostart',
            ],
        });
    });

    it('marks automatic single-file tab sync as exact-visible to avoid stale sibling resurrection', () => {
        const planned = buildTabChangeCommand({
            activeFile: 'tasks/software/tsift.md',
            visibleMd: ['tasks/software/tsift.md'],
            visibleColumns: [['tasks/software/tsift.md']],
            previous: {
                activeFile: 'tasks/software/corky.md',
                visibleSignature: visibleSignatureFromColumns([
                    ['tasks/software/tsift.md'],
                    ['tasks/software/corky.md'],
                ]),
            },
        });

        assert.deepStrictEqual(planned?.command, {
            kind: 'sync',
            args: [
                'sync',
                '--col',
                'tasks/software/tsift.md',
                '--focus',
                'tasks/software/tsift.md',
                '--exact-visible',
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

    it('builds immediate focus command args for the fast tab handoff path', () => {
        assert.deepStrictEqual(
            buildImmediateFocusCommandArgs('tasks/agent-doc/agent-doc-bugs2.md'),
            ['focus', 'tasks/agent-doc/agent-doc-bugs2.md', '--no-stash-promote'],
        );
    });

    it('replays the latest queued tab change after a running sync finishes', () => {
        assert.strictEqual(shouldReplayQueuedTabChange(3, 4), true);
        assert.strictEqual(shouldReplayQueuedTabChange(4, 4), false);
    });

    it('does not schedule deferred retry work for a superseded tab sync', () => {
        assert.strictEqual(shouldScheduleDeferredTabSyncRetry(3, 4), false);
        assert.strictEqual(shouldScheduleDeferredTabSyncRetry(4, 4), true);
    });

    it('keeps passive preserve-layout sync pending for retry', () => {
        const result = analyzeTabSyncCommandResult(
            {
                kind: 'sync',
                args: ['sync', '--col', 'tasks/software/tsift.md', '--focus', 'tasks/software/tsift.md', '--no-autostart'],
            },
            0,
            '[sync] safe passive sync preserved the current tmux layout because missing requested pane(s) tasks/software/tsift.md while visible protected pane(s) %210:preflight_started:tasks/agent-doc/agent-doc-bugs2.md cannot be detached safely because those panes still own open closeout cycle(s)',
        );

        assert.deepStrictEqual(result, { applied: false, shouldRetry: true });
    });

    it('keeps safe-passive sync lock contention pending for the latest retry', () => {
        const result = analyzeTabSyncCommandResult(
            {
                kind: 'sync',
                args: ['sync', '--col', 'tasks/software/tsift.md', '--focus', 'tasks/software/tsift.md', '--no-autostart'],
            },
            0,
            '[sync] safe_passive_sync_lock_contention_retry phase=sync_lock_wait elapsed_ms=101 budget_ms=100 status=over_budget coalesced=skipped_stale action=retry',
        );

        assert.deepStrictEqual(result, { applied: false, shouldRetry: true });
    });

    it('treats preserve-layout sync as applied when the focused pane was reselected', () => {
        const result = analyzeTabSyncCommandResult(
            {
                kind: 'sync',
                args: ['sync', '--col', 'tasks/software/tsift.md', '--focus', 'tasks/software/tsift.md', '--no-autostart'],
            },
            0,
            [
                '[sync] safe passive sync preserved the current tmux layout because unresolved files remain blocked: tasks/software/tsift.md',
                '[sync] safe_passive_layout_preserved_reselected_focus pane=%202 reason=blocked_files',
            ].join('\n'),
        );

        assert.deepStrictEqual(result, { applied: true, shouldRetry: false });
    });

    it('treats focus success as applied', () => {
        const result = analyzeTabSyncCommandResult(
            { kind: 'focus', args: ['focus', 'tasks/software/tsift.md', '--no-stash-promote'] },
            0,
            '',
        );

        assert.deepStrictEqual(result, { applied: true, shouldRetry: false });
    });
});
