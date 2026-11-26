import { describe, it } from 'node:test';
import assert from 'node:assert';
import fs from 'node:fs';
import {
    acceptableControlPath,
    buildArtifactInitArgs,
    defaultControlSessionPath,
} from './claimArtifact.js';

describe('claimArtifact', () => {
    it('defaults the control session to a sibling stem.session.md', () => {
        assert.strictEqual(defaultControlSessionPath('plans/my-plan.md'), 'plans/my-plan.session.md');
        assert.strictEqual(defaultControlSessionPath('./README.md'), 'README.session.md');
        assert.strictEqual(defaultControlSessionPath('notes.MD'), 'notes.session.md');
    });

    it('refuses the artifact path, a non-markdown path, and an absolute path', () => {
        assert.strictEqual(acceptableControlPath('plan.session.md', 'plan.md'), true);
        assert.strictEqual(acceptableControlPath('plan.md', './plan.md'), false);
        assert.strictEqual(acceptableControlPath('notes.txt', 'plan.md'), false);
        assert.strictEqual(acceptableControlPath('/tmp/plan.session.md', 'plan.md'), false);
    });

    it('inits the control session aimed at the artifact', () => {
        assert.deepStrictEqual(
            buildArtifactInitArgs('plan.session.md', 'plan.md'),
            ['init', 'plan.session.md', '--artifact', 'plan.md'],
        );
    });

    it('publishes stable command ids and the new titles', () => {
        const pkg = JSON.parse(fs.readFileSync(new URL('../package.json', import.meta.url), 'utf8')) as {
            contributes: { commands: { command: string; title: string }[] };
        };
        const byId = new Map(pkg.contributes.commands.map(command => [command.command, command.title]));
        assert.strictEqual(byId.get('agentDoc.claim'), 'Agent Doc: Claim Control Session');
        assert.strictEqual(byId.get('agentDoc.forceClaim'), 'Agent Doc: Force Claim Control Session');
        assert.strictEqual(byId.get('agentDoc.claimArtifact'), 'Agent Doc: Claim Artifact Session');
        assert.strictEqual([...byId.values()].some(title => title.includes('Claim Session File')), false);
    });
});
