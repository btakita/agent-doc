import { test } from 'node:test';
import * as assert from 'node:assert/strict';
import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import * as crypto from 'crypto';
import {
    ACTIVATION_RECEIPT_PREFIX,
    currentWorkspaceActivationReceipt,
    encodeWorkspaceActivationReceipt,
    fileSha256,
    recordActivatedShell,
} from './activationReceipt.js';

// The same line `agent_doc_reliable_sync_io::liveness` parses in
// `workspace_activation_receipt_round_trips_through_the_registration`.
test('workspace receipt encodes the shared agent_doc_activation_receipt/v1 line', () => {
    assert.equal(
        encodeWorkspaceActivationReceipt({
            instance: '6f1c',
            shell: 'ab12',
            implVersion: '0.35.500',
            implSha256: 'cd34',
            generation: 2,
        }),
        'agent_doc_activation_receipt/v1 role=workspace jvm=6f1c shell=ab12 spi=0 '
            + 'impl_version=0.35.500 impl_sha256=cd34 generation=2 epoch=2',
    );
});

test('workspace receipt reports none before the native library loads', () => {
    const line = encodeWorkspaceActivationReceipt({
        instance: 'u',
        shell: null,
        implVersion: null,
        implSha256: '  ',
        generation: 0,
    });
    assert.equal(
        line,
        'agent_doc_activation_receipt/v1 role=workspace jvm=u shell=none spi=0 '
            + 'impl_version=none impl_sha256=none generation=0 epoch=0',
    );
});

test('a receipt value never splits into two fields', () => {
    const line = encodeWorkspaceActivationReceipt({
        instance: 'a b',
        shell: 's',
        implVersion: '0.35 dev',
        implSha256: null,
        generation: 1.7,
    });
    const fields = line.slice(ACTIVATION_RECEIPT_PREFIX.length + 1).split(' ');
    assert.deepEqual(fields.map((field) => field.split('=')[0]), [
        'role', 'jvm', 'shell', 'spi', 'impl_version', 'impl_sha256', 'generation', 'epoch',
    ]);
    assert.ok(fields.every((field) => /^[a-z_0-9]+=\S+$/.test(field)), line);
    assert.ok(line.includes('jvm=a_b') && line.includes('impl_version=0.35_dev'), line);
    assert.ok(line.endsWith('generation=1 epoch=1'), line);
});

test('the shell digest is the sha256 of the bundle this host loaded', () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'agent-doc-receipt-'));
    try {
        const bundle = path.join(dir, 'extension.js');
        fs.writeFileSync(bundle, 'export function activate() {}\n');
        const digest = crypto.createHash('sha256').update(fs.readFileSync(bundle)).digest('hex');
        assert.equal(fileSha256(bundle), digest);
        assert.equal(fileSha256(path.join(dir, 'missing.js')), null);

        recordActivatedShell(bundle);
        // A VSIX installed after activation must not change the running receipt.
        fs.writeFileSync(bundle, 'export function activate() { /* newer */ }\n');
        const line = currentWorkspaceActivationReceipt({ version: '1.0.0', sha256: 'ff', generation: 1 });
        assert.match(line, new RegExp(` shell=${digest} `));
        assert.match(line, / impl_version=1\.0\.0 impl_sha256=ff generation=1 epoch=1$/);
    } finally {
        fs.rmSync(dir, { recursive: true, force: true });
    }
});
