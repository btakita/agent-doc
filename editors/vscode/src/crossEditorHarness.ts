import * as readline from 'readline';
import { createHash } from 'crypto';
import {
    ControllerSocketReplicaTransport,
    CrdtReplicaForwarder,
    type ReplicaResumeState,
} from './crdtReplica.js';
import { NativeReplicaNode } from './native.js';

export const CROSS_EDITOR_NATIVE_HARNESS_CAPABILITY = 'cross_editor_native_harness_v1';

type HarnessCommand = {
    command: 'attach' | 'edit' | 'pull' | 'disconnect' | 'reconnect' | 'text' | 'shutdown';
    offset?: number;
    deleteLen?: number;
    insert?: string;
};

function requiredEnv(name: string): string {
  const value = process.env[name];
  if (!value) {
    throw new Error(`${name} is required`);
  }
  return value;
}

const projectRoot = requiredEnv('AGENT_DOC_HARNESS_PROJECT_ROOT');
const filePath = requiredEnv('AGENT_DOC_HARNESS_FILE');
const identity = process.env.AGENT_DOC_HARNESS_IDENTITY ?? 'vscode:native-harness';

let forwarder: CrdtReplicaForwarder | null = null;
let retained: ReplicaResumeState | null = null;

function newForwarder(resumeState: ReplicaResumeState | null): CrdtReplicaForwarder {
    return new CrdtReplicaForwarder(
        filePath,
        identity,
        new NativeReplicaNode(projectRoot),
        new ControllerSocketReplicaTransport(projectRoot),
        resumeState,
    );
}

function reply(payload: Record<string, unknown>): void {
    process.stdout.write(`${JSON.stringify({ harness: 'vscode', ...payload })}\n`);
}

async function handle(command: HarnessCommand): Promise<boolean> {
    switch (command.command) {
        case 'attach': {
            forwarder = newForwarder(null);
            const registered = await forwarder.register();
            reply({ ok: registered, text: forwarder.replicaText() });
            return true;
        }
        case 'edit': {
            if (!forwarder) throw new Error('harness is not attached');
            await forwarder.forwardLocalDelta(
                command.offset ?? 0,
                command.deleteLen ?? 0,
                command.insert ?? '',
            );
            reply({ ok: true, text: forwarder.replicaText() });
            return true;
        }
        case 'pull': {
            if (!forwarder) throw new Error('harness is not attached');
      const updates = await forwarder.pullRemoteUpdates();
      let allAcked = true;
      const receipts: Array<Record<string, unknown>> = [];
      for (const update of updates) {
        const text = forwarder.applyRemoteUpdate(update.update);
        const projected = text == null
          ? false
          : await forwarder.projectVisibleState(text);
        receipts.push({
          patchId: update.patchId,
          generation: update.generation,
          expectedContentHash: update.expectedContentHash,
          appliedContentHash: text == null
            ? null
            : createHash('sha256').update(text, 'utf8').digest('hex'),
          projected,
        });
        allAcked = projected && allAcked;
      }
      reply({
        ok: allAcked,
        applied: updates.length,
        receipts,
        text: forwarder.replicaText(),
      });
            return true;
        }
        case 'disconnect': {
            if (!forwarder) throw new Error('harness is not attached');
            retained = forwarder.captureResumeState();
            await forwarder.deregister();
            forwarder = null;
            reply({ ok: retained !== null });
            return true;
        }
        case 'reconnect': {
            if (!retained) throw new Error('harness has no retained replica state');
            forwarder = newForwarder(retained);
            const registered = await forwarder.register();
            reply({ ok: registered, text: forwarder.replicaText() });
            return true;
        }
        case 'text': {
            reply({ ok: forwarder !== null, text: forwarder?.replicaText() ?? null });
            return true;
        }
        case 'shutdown': {
            if (forwarder) await forwarder.deregister();
            forwarder = null;
            reply({ ok: true });
            return false;
        }
    }
}

const input = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
for await (const line of input) {
  if (line.trim().length === 0) continue;
  try {
        const keepRunning = await handle(JSON.parse(line) as HarnessCommand);
        if (!keepRunning) break;
    } catch (error: any) {
    reply({ ok: false, error: error?.stack ?? error?.message ?? String(error) });
  }
}
input.close();
