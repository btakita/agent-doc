import { Delta, DeltaOpCellSet, IpcValueInline, NodeSnapshot, NodeStatePayload, Snapshot } from '@lazily-hub/lazily-js';
import { randomUUID } from 'node:crypto';
import type { RepoContextAnchor, RepoContextLink } from './repoContextModel.js';

export type ExplorerAction = 'search' | 'select' | 'openLink' | 'ask' | 'openSource' | 'chooseSession' | 'openSession' | 'openAnswer';
type EventKind = 'activate' | 'change' | 'submit';
type Scalar = string | number | boolean | null;

export interface ExplorerViewFrame {
    version: 1;
    view_instance: string;
    displayed_revision: number;
    node_id: string;
    event_id: string;
    event: EventKind;
    payload: Scalar;
}

interface ViewNode {
    role: 'region' | 'field' | 'list' | 'text' | 'button';
    children?: string[];
    text?: string | { op: string; args: string[] };
    disabled?: boolean;
}

export interface ExplorerViewTemplate {
    definition: {
        id: string;
        root: string;
        signals: Record<string, { initial: Scalar }>;
        nodes: Record<string, ViewNode>;
    };
    bindings: Record<string, number>;
    actions: Record<string, Partial<Record<EventKind, ExplorerAction>>>;
}

interface Revision extends ExplorerViewTemplate {
    version: 1;
    view_instance: string;
    revision: number;
}

type Outcome = { status: 'accepted'; action: ExplorerAction } |
    { status: 'denied' | 'duplicate' | 'stale' | 'conflict' | 'unavailable' | 'wrong-instance' };

interface Receiver {
    readonly signals: Readonly<Record<string, Scalar>>;
    attach(revision: Revision, snapshot: Snapshot): string;
    replace(revision: Revision): string;
    applyDelta(delta: Delta): string;
    routeEvent(frame: ExplorerViewFrame, allow: (action: ExplorerAction, event: ExplorerViewFrame) => boolean): Outcome;
}

type Runtime = { RealtimeView: new (instance: string) => Receiver };

// The bundle injects this import only when the candidate binding is installed.
declare const __agentDocRealtimeViewRuntime: (() => Promise<Runtime>) | undefined;

export async function loadRepoContextViewRuntime(): Promise<Runtime | undefined> {
    return typeof __agentDocRealtimeViewRuntime === 'function' ? __agentDocRealtimeViewRuntime() : undefined;
}

export function explorerRows<T extends RepoContextAnchor | RepoContextLink>(
    group: 'results' | 'related', values: T[],
): Array<T & { id: string }> {
    const keyed = new Map(values.map(item => [
        group + ':' + JSON.stringify([
            item.file, item.line, item.endLine, item.name, 'kind' in item ? item.kind : item.relation,
        ]),
        item,
    ]));
    return [...keyed].map(([id, item]) => ({ ...item, id }));
}

type KeyedRow = { id: string; name: string };

export class RepoContextRealtimeView {
    private readonly instance = randomUUID();
    private readonly runtime: Receiver;
    private current: Revision | undefined;
    private epoch = 0;
    private count = 0;

    constructor(runtime: Runtime, private readonly template: ExplorerViewTemplate) {
        this.runtime = new runtime.RealtimeView(this.instance);
    }

    publish(
        results: KeyedRow[], related: KeyedRow[],
        available: { selection: boolean; session: boolean; answer: boolean },
        reconnect = false,
    ): object[] {
        const body = structuredClone(this.template);
        body.definition.nodes['submit-question'].disabled = !available.selection;
        body.definition.nodes['open-source'].disabled = !available.selection;
        body.definition.nodes['open-session'].disabled = !available.session;
        body.definition.nodes['open-answer'].disabled = !available.answer;
        for (const [group, rows, action] of [
            ['results', results, 'select'], ['related', related, 'openLink'],
        ] as const) {
            body.definition.nodes[group].children = rows.map(row => row.id);
            for (const row of rows) {
                body.definition.nodes[row.id] = { role: 'button', text: row.name };
                body.actions[row.id] = { activate: action };
            }
        }
        const previous = this.current;
        const bindingNode = body.bindings.relatedCount;
        const tag = 'lazily.view.scalar.v1';
        const payload = [...Buffer.from(JSON.stringify(related.length), 'utf8')];
        const messages: object[] = [];
        if (previous && this.count !== related.length) {
            const delta = Delta.next(this.epoch, [new DeltaOpCellSet(bindingNode, new IpcValueInline(payload))]);
            this.runtime.applyDelta(delta);
            this.epoch = delta.epoch;
            if (!reconnect) messages.push({ type: 'view-delta', delta: { Delta: delta.toWire() } });
        }
        this.count = related.length;
        const candidate: Revision = {
            ...body, version: 1, view_instance: this.instance, revision: previous?.revision ?? 1,
        };
        const changed = !previous || JSON.stringify(candidate) !== JSON.stringify(previous);
        if (previous && changed) candidate.revision++;
        if (!previous || reconnect) {
            const snapshot = new Snapshot({
                epoch: this.epoch,
                nodes: [new NodeSnapshot(bindingNode, tag, new NodeStatePayload(payload))],
                roots: [bindingNode],
            });
            this.runtime.attach(candidate, snapshot);
            messages.push({ type: 'view', revision: candidate, snapshot: { Snapshot: snapshot.toWire() } });
        } else if (changed) {
            this.runtime.replace(candidate);
            messages.push({ type: 'view', revision: candidate });
        }
        this.current = candidate;
        if (messages.length > 0) messages.push({ type: 'view-values', relatedCount: this.runtime.signals.relatedCount });
        return messages;
    }

    authorize(frame: ExplorerViewFrame, allow: (action: ExplorerAction, event: ExplorerViewFrame) => boolean): Outcome {
        return this.runtime.routeEvent(frame, allow);
    }
}
