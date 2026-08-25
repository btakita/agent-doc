import assert from 'node:assert/strict';
import test from 'node:test';

import {
    decideIdeTerminalAttach,
    IdeTerminalAttachDecision,
    parseTmuxEnsureReceipt,
} from './ideTerminal.js';

test('parseTmuxEnsureReceipt validates and maps the CLI JSON receipt', () => {
    assert.deepEqual(parseTmuxEnsureReceipt(JSON.stringify({
        session_name: 'agent-doc-project',
        pane_id: '%7',
        attach_command: "tmux attach-session -t 'agent-doc-project'",
            created: true,
            attached: false,
            terminal_host: 'ide',
            terminal_host_reason: 'configured IDE host',
            auto_start_tmux: true,
    })), {
        sessionName: 'agent-doc-project',
        paneId: '%7',
        attachCommand: "tmux attach-session -t 'agent-doc-project'",
        created: true,
            attached: false,
            terminalHost: 'ide',
            terminalHostReason: 'configured IDE host',
            autoStartTmux: true,
    });

    assert.throws(
        () => parseTmuxEnsureReceipt('{"session_name":"missing-fields"}'),
        /invalid receipt/,
    );
});

test('attached external tmux client is left alone', () => {
    assert.equal(
        decideIdeTerminalAttach('none', true, false),
        IdeTerminalAttachDecision.NoopExternalAttached,
    );
});

test('live agent-doc terminals are focused or reused', () => {
    assert.equal(
        decideIdeTerminalAttach('none', true, true),
        IdeTerminalAttachDecision.FocusExisting,
    );
    assert.equal(
        decideIdeTerminalAttach('ide', false, true),
        IdeTerminalAttachDecision.AttachExisting,
    );
});

test('a detached session without an editor terminal creates one', () => {
    assert.equal(
        decideIdeTerminalAttach('ide', false, false),
        IdeTerminalAttachDecision.CreateAndAttach,
    );
});

test('non-IDE host policy does not open an editor terminal', () => {
    assert.equal(
        decideIdeTerminalAttach('external', false, false),
        IdeTerminalAttachDecision.NoopConfiguredHost,
    );
    assert.equal(
        decideIdeTerminalAttach('none', false, true),
        IdeTerminalAttachDecision.NoopConfiguredHost,
    );
});
