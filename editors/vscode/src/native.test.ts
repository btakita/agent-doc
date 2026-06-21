import { describe, it } from 'node:test';
import assert from 'node:assert';
import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import { libMtimeChanged, writePidLock, removePidLock, parseReconnectDecision } from './native';

describe('parseReconnectDecision (#yzer reconnect-reread, VS Code/JB parity)', () => {
    it('parses a reread_disk decision with content', () => {
        const json = JSON.stringify({ decision: 'reread_disk', content: 'disk text\n' });
        assert.deepStrictEqual(parseReconnectDecision(json), {
            decision: 'reread_disk',
            content: 'disk text\n',
        });
    });

    it('parses a keep_buffer decision without content', () => {
        assert.deepStrictEqual(parseReconnectDecision(JSON.stringify({ decision: 'keep_buffer' })), {
            decision: 'keep_buffer',
        });
    });

    it('returns null on malformed JSON (fail safe — keep buffer)', () => {
        assert.strictEqual(parseReconnectDecision('{not json'), null);
    });

    it('returns null when the decision field is missing or non-string', () => {
        assert.strictEqual(parseReconnectDecision(JSON.stringify({ content: 'x' })), null);
        assert.strictEqual(parseReconnectDecision(JSON.stringify({ decision: 5 })), null);
    });

    it('ignores a non-string content field', () => {
        const out = parseReconnectDecision(JSON.stringify({ decision: 'reread_disk', content: 42 }));
        assert.deepStrictEqual(out, { decision: 'reread_disk' });
    });
});

describe('libMtimeChanged', () => {
    it('returns false when mtime unchanged', () => {
        const tmp = path.join(os.tmpdir(), `libagent_doc_test_${Date.now()}.so`);
        fs.writeFileSync(tmp, 'content');
        try {
            const mtime = fs.statSync(tmp).mtimeMs;
            assert.strictEqual(libMtimeChanged(tmp, mtime), false);
        } finally {
            fs.unlinkSync(tmp);
        }
    });

    it('returns true when file modified', async () => {
        const tmp = path.join(os.tmpdir(), `libagent_doc_test_${Date.now()}.so`);
        fs.writeFileSync(tmp, 'original');
        try {
            const oldMtime = fs.statSync(tmp).mtimeMs;
            await new Promise(resolve => setTimeout(resolve, 1100));
            fs.writeFileSync(tmp, 'updated');
            assert.strictEqual(libMtimeChanged(tmp, oldMtime), true);
        } finally {
            fs.unlinkSync(tmp);
        }
    });

    it('returns false for nonexistent file', () => {
        assert.strictEqual(
            libMtimeChanged(`/tmp/nonexistent_lib_${Date.now()}.so`, 99999),
            false
        );
    });

    it('returns false when storedMtime matches current', () => {
        const tmp = path.join(os.tmpdir(), `libagent_doc_test_${Date.now()}.so`);
        fs.writeFileSync(tmp, 'content');
        try {
            const mtime = fs.statSync(tmp).mtimeMs;
            assert.strictEqual(libMtimeChanged(tmp, mtime), false);
        } finally {
            fs.unlinkSync(tmp);
        }
    });
});

describe('pidLock', () => {
    it('writePidLock creates lock file and removePidLock removes it', () => {
        const tmp = path.join(os.tmpdir(), `libagent_doc_test_${Date.now()}.so`);
        fs.writeFileSync(tmp, 'content');
        const expectedLock = `${tmp}.pid.${process.pid}`;
        try {
            writePidLock(tmp);
            assert.strictEqual(fs.existsSync(expectedLock), true);
            removePidLock();
            assert.strictEqual(fs.existsSync(expectedLock), false);
        } finally {
            try { fs.unlinkSync(tmp); } catch {}
            try { fs.unlinkSync(expectedLock); } catch {}
        }
    });

    it('removePidLock is safe to call without prior write', () => {
        assert.doesNotThrow(() => removePidLock());
    });
});
