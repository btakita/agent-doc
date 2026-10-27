import { describe, it } from 'node:test';
import assert from 'node:assert';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import {
    SESSION_COMMAND_OUTCOME_DERIVATION,
    combinedCommandOutput,
    deriveSessionCommandOutcome,
    deriveSessionCommandOutcomeFloor,
    type SessionCommandOutcomeEvidence,
} from './sessionCommandOutcome.js';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

interface ParityCase {
    name: string;
    input: SessionCommandOutcomeEvidence;
    expected: Record<string, unknown>;
    /** A deliberate, documented divergence of the frozen floor from native. */
    frozen_expected?: Record<string, unknown>;
}

/**
 * The one shared parity file: the Rust derivation (agent-doc-editor-surface-io),
 * the JetBrains Kotlin floor (SessionCommandOutcomeTest), and this floor all
 * read it in place, so none of them can drift silently.
 */
function parityCases(): ParityCase[] {
    const fixturePath = path.join(
        __dirname,
        '..',
        '..',
        'jetbrains',
        'src',
        'test',
        'resources',
        'editor-derivations',
        'session_command_outcome.json',
    );
    const fixture = JSON.parse(fs.readFileSync(fixturePath, 'utf-8'));
    assert.strictEqual(fixture.derivation, SESSION_COMMAND_OUTCOME_DERIVATION);
    return fixture.cases as ParityCase[];
}

function evidence(
    command: 'clear' | 'restart',
    succeeded: boolean,
    output: string,
    force = false,
): SessionCommandOutcomeEvidence {
    return { command, succeeded, force, output, relative_path: 'tasks/root.md' };
}

describe('session_command_outcome frozen floor', () => {
    it('answers every shared parity case the native derivation answers', () => {
        const cases = parityCases();
        assert.ok(cases.length >= 51, 'the parity file lost cases');
        for (const parity of cases) {
            const expected = parity.frozen_expected ?? parity.expected;
            assert.deepStrictEqual(
                deriveSessionCommandOutcomeFloor(parity.input),
                expected,
                parity.name,
            );
        }
    });

    it('reads the pre-move VS Code busy clear refusal shapes', () => {
        const busy = deriveSessionCommandOutcomeFloor(evidence(
            'clear',
            false,
            'agent-doc command failed (exit 1): Error: session_clear refused for /repo/tasks/root.md because pane %2 is alive-busy (source=authoritative_actor, current_command=agent-doc, tail="Tip says \\"Override\\" settings (per agent)"). Run `agent-doc session status /repo/tasks/root.md` and wait for an idle prompt before retrying `agent-doc session clear`, or run `agent-doc session interrupt-clear /repo/tasks/root.md` to intentionally interrupt the pane and clear context.',
        ));
        assert.strictEqual(busy.kind, 'clear_refused');
        assert.deepStrictEqual(busy.clear_refused, {
            file: '/repo/tasks/root.md',
            pane: '%2',
            source: 'authoritative_actor',
            current_command: 'agent-doc',
            tail: 'Tip says "Override" settings (per agent)',
            protected_reason: '',
            live_turn_evidence: '',
        });
        assert.match(busy.summary!, /^Session is still running for tasks\/root\.md\.\n/);
        assert.match(busy.summary!, /Pane %2 is busy \(agent-doc\)/);
        assert.match(busy.summary!, /Refresh and retry/);
        assert.match(busy.summary!, /Interrupt and clear/);

        const protectedInput = deriveSessionCommandOutcomeFloor(evidence(
            'clear',
            false,
            'Error: session_clear refused for /repo/tasks/root.md because pane %2 contains protected prompt input (reason=drafted prompt input, source=authoritative_actor, current_command=agent-doc, tail="› unfinished prompt"). Clear the prompt input manually, or run `agent-doc session interrupt-clear /repo/tasks/root.md` to intentionally interrupt the pane and clear context.',
        ));
        assert.strictEqual(protectedInput.kind, 'clear_refused');
        assert.strictEqual(protectedInput.clear_refused?.protected_reason, 'drafted prompt input');
        assert.strictEqual(protectedInput.clear_refused?.tail, '› unfinished prompt');
        assert.match(protectedInput.summary!, /protected prompt input/);
        assert.match(protectedInput.summary!, /Interrupt and clear/);
        assert.doesNotMatch(protectedInput.summary!, /Refresh and retry/);
    });

    it('reads the pre-move VS Code restart refusal shapes', () => {
        const busy = deriveSessionCommandOutcomeFloor(evidence(
            'restart',
            false,
            'agent-doc command failed (exit 1): Error: session_restart refused for /repo/tasks/root.md because pane %2 is alive-busy (source=authoritative_actor, current_command=agent-doc, tail="gpt-5 high - ~/repo - Context 20% used"). Run `agent-doc session status /repo/tasks/root.md` and wait for an idle prompt, or pass `--force` to interrupt the running turn and restart anyway.',
        ));
        assert.strictEqual(busy.kind, 'restart_refused_busy');
        assert.deepStrictEqual(busy.restart_refused_busy, {
            file: '/repo/tasks/root.md',
            pane: '%2',
            source: 'authoritative_actor',
            current_command: 'agent-doc',
            tail: 'gpt-5 high - ~/repo - Context 20% used',
        });
        assert.match(busy.summary!, /Restart Supervisor is blocked/);
        assert.match(busy.summary!, /Interrupt and restart/);

        const startingOutput = 'agent-doc command failed (exit 1): Error: session_restart refused for /repo/tasks/root.md because the authoritative actor is still starting and the document changed after the last committed cycle. Wait for a dispatch-ready prompt (`prompt_ready=true`) and retry, or run `agent-doc session status /repo/tasks/root.md` to inspect the pane. Pass `--force` to interrupt the running turn and restart anyway.';
        const starting = deriveSessionCommandOutcomeFloor(evidence('restart', false, startingOutput));
        assert.strictEqual(starting.kind, 'restart_refused_starting');
        assert.deepStrictEqual(starting.restart_refused_starting, {
            file: '/repo/tasks/root.md',
            reason: 'the document changed after the last committed cycle',
        });
        assert.match(starting.summary!, /authoritative actor is still starting/);
        // Interrupt and restart is the remedy, so a forced restart never reads it again.
        assert.strictEqual(deriveSessionCommandOutcomeFloor(evidence('restart', false, startingOutput, true)).kind, 'generic');
    });

    it('reads ASCII \\s inside a character class, as java.util.regex does', () => {
        // `[^,\s)]+` stops the live-turn evidence at an ASCII space but not at a no-break space.
        const outcome = deriveSessionCommandOutcomeFloor(evidence(
            'clear',
            true,
            'session_clear queued for /r/doc.md — a turn is live in pane %1 (harness_turn_live=true evidence=a b c)',
        ));
        assert.strictEqual(outcome.clear_queued?.evidence, 'a b');
    });
});

describe('session_command_outcome native answer', () => {
    const queuedOutput =
        'session_clear queued for /repo/tasks/root.md — a turn is live in pane %4 (harness_turn_live=true evidence=turn_active_lease, source=authoritative_actor)';

    it('sends the evidence and prefers the native answer', () => {
        const input = evidence('clear', true, queuedOutput);
        let request: unknown;
        const nativeOutcome = {
            ...deriveSessionCommandOutcomeFloor(input),
            summary: 'native wording',
        };
        const answer = deriveSessionCommandOutcome(input, (json) => {
            request = JSON.parse(json);
            return JSON.stringify({ derivation: SESSION_COMMAND_OUTCOME_DERIVATION, output: nativeOutcome });
        });
        assert.deepStrictEqual(request, { derivation: SESSION_COMMAND_OUTCOME_DERIVATION, input });
        assert.strictEqual(answer.native, true);
        assert.strictEqual(answer.outcome.kind, 'clear_queued');
        assert.strictEqual(answer.outcome.summary, 'native wording');
        assert.strictEqual(answer.outcome.clear_queued?.pane, '%4');
    });

    it('keeps the floor for a library that cannot answer', () => {
        const input = evidence('clear', true, queuedOutput);
        const floor = deriveSessionCommandOutcomeFloor(input);
        for (const unanswered of [
            () => null,
            () => 'not json',
            () => JSON.stringify({ derivation: 'capabilities', output: {} }),
            () => JSON.stringify({ derivation: SESSION_COMMAND_OUTCOME_DERIVATION, output: null }),
            () => { throw new Error('unknown editor derivation'); },
        ]) {
            const answer = deriveSessionCommandOutcome(input, unanswered);
            assert.strictEqual(answer.native, false);
            assert.deepStrictEqual(answer.outcome, floor);
        }
        assert.deepStrictEqual(deriveSessionCommandOutcome(input).outcome, floor);
    });

    it('takes the whole floor for a kind this generation cannot execute or a kind without its details', () => {
        const input = evidence('clear', true, queuedOutput);
        const floor = deriveSessionCommandOutcomeFloor(input);
        for (const output of [
            { kind: 'clear_postponed_forever', summary: 'new kind' },
            { kind: 'clear_queued', summary: 'no details', clear_queued: null },
            { kind: 'clear_refused', summary: 'no details' },
        ]) {
            const answer = deriveSessionCommandOutcome(
                input,
                () => JSON.stringify({ derivation: SESSION_COMMAND_OUTCOME_DERIVATION, output }),
            );
            assert.strictEqual(answer.native, false, output.kind);
            assert.deepStrictEqual(answer.outcome, floor, output.kind);
        }
    });

    it('fills a summary an older library leaves out from the floor', () => {
        const input = evidence('clear', true, '');
        const answer = deriveSessionCommandOutcome(
            input,
            () => JSON.stringify({ derivation: SESSION_COMMAND_OUTCOME_DERIVATION, output: { kind: 'cleared' } }),
        );
        assert.strictEqual(answer.native, true);
        assert.strictEqual(answer.outcome.summary, 'Cleared session context for tasks/root.md');
    });
});

describe('combinedCommandOutput', () => {
    it('keeps a queued clear the binary reports on stderr while exiting zero', () => {
        assert.strictEqual(combinedCommandOutput('', `  ${queuedLine()}\n`), queuedLine());
        assert.strictEqual(combinedCommandOutput(' Cleared. ', ' warning '), 'Cleared.\nwarning');
        assert.strictEqual(combinedCommandOutput('', ''), '');
    });
});

function queuedLine(): string {
    return 'session_clear deferred for /repo/tasks/root.md — pane %2 is alive-busy under an active `agent:queue auto` loop';
}
