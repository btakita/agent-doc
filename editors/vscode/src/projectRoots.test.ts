import * as assert from 'node:assert/strict';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { test } from 'node:test';
import { ProjectRoots } from './projectRoots.js';

test('root discovery respects the home, temp and configured ceiling boundaries', () => {
    const fixture = fs.mkdtempSync(path.join(os.tmpdir(), 'agent-doc-root-policy-'));
    try {
        const home = path.join(fixture, 'home');
        const temp = path.join(fixture, 'temp');
        const notes = path.join(home, 'notes');
        const child = path.join(notes, 'work');
        for (const dir of [home, temp, notes]) {
            fs.mkdirSync(path.join(dir, '.agent-doc'), { recursive: true });
        }
        fs.mkdirSync(child);

        const policy = new ProjectRoots(home, [temp], [notes], false);
        assert.equal(policy.find(home), undefined);
        assert.equal(policy.find(temp), undefined);
        assert.equal(policy.find(notes), notes);
        assert.equal(policy.find(child), undefined);
        assert.equal(policy.allowsFallback(child, home), false);
        assert.equal(policy.allowsFallback(child, notes), false);
        assert.equal(policy.allowsFallback(notes, notes), true);
        assert.equal(policy.allows(path.parse(home).root), false);

        assert.equal(new ProjectRoots(home, [temp], [], true).find(home), home);
        fs.mkdirSync(path.join(home, '.git'));
        assert.equal(policy.find(home), home);
    } finally {
        fs.rmSync(fixture, { recursive: true, force: true });
    }
});
