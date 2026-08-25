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
    })), {
        sessionName: 'agent-doc-project',
        paneId: '%7',
        attachCommand: "tmux attach-session -t 'agent-doc-project'",
        created: true,
        attached: false,
    });

    assert.throws(
        () => parseTmuxEnsureReceipt('{"session_name":"missing-fields"}'),
        /invalid receipt/,
    );
});

test('attached external tmux client is left alone', () => {
    assert.equal(
        decideIdeTerminalAttach(true, false),
        IdeTerminalAttachDecision.NoopExternalAttached,
    );
});

test('live agent-doc terminals are focused or reused', () => {
    assert.equal(
        decideIdeTerminalAttach(true, true),
        IdeTerminalAttachDecision.FocusExisting,
    );
    assert.equal(
        decideIdeTerminalAttach(false, true),
        IdeTerminalAttachDecision.AttachExisting,
    );
});

test('a detached session without an editor terminal creates one', () => {
    assert.equal(
        decideIdeTerminalAttach(false, false),
        IdeTerminalAttachDecision.CreateAndAttach,
    );
});
