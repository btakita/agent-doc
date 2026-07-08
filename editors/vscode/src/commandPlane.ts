// #lzmsgpcp: lazily command/RPC message plane (`command-plane-v1`) helpers for
// the VS Code `Run Agent Doc` action.
//
// Builds an `agent-doc.editor_route.v1` `CommandSubmit` envelope and resolves a
// unary `call` ONLY on a terminal `applied` projection. A transport ACK or an
// `accepted`/`started` progress event never resolves the call — the controller's
// shadow endpoint returns after folding the terminal causal receipt.
//
// The envelope shape mirrors lazily-js `CommandSubmit.toWire()` / the
// `schemas/message-passing.json` contract so the controller (and the other
// bindings) decode it identically.

import * as crypto from 'crypto';

export interface EditorRoutePayload {
    source: string;
    relative_path: string;
    dispatch_only: boolean;
    plain_trigger: boolean;
    wait_for_ready_secs: number;
    layout_args: string[];
    route_key: string;
}

export function buildEditorRoutePayload(
    rel: string,
    routeKey: string,
    layoutArgs: string[],
    waitForReadySecs: number,
): EditorRoutePayload {
    return {
        source: 'vscode_plugin',
        relative_path: rel,
        dispatch_only: true,
        plain_trigger: true,
        wait_for_ready_secs: waitForReadySecs,
        layout_args: layoutArgs,
        route_key: routeKey,
    };
}

export interface EditorRouteCommand {
    commandId: string;
    message: { CommandSubmit: Record<string, unknown> };
}

// Build the `CommandSubmit` envelope carrying an inline `editor_route` payload.
export function buildEditorRouteCommandMessage(
    rel: string,
    routeKey: string,
    layoutArgs: string[],
    waitForReadySecs: number,
    commandId: string = `cmd-${crypto.randomUUID()}`,
): EditorRouteCommand {
    const payloadBytes = Buffer.from(
        JSON.stringify(buildEditorRoutePayload(rel, routeKey, layoutArgs, waitForReadySecs)),
        'utf8',
    );
    const payloadHash = 'sha256:' + crypto.createHash('sha256').update(payloadBytes).digest('hex');
    return {
        commandId,
        message: {
            CommandSubmit: {
                command_id: commandId,
                causation_id: commandId,
                source: 'vscode-plugin',
                target: 'project-controller',
                namespace: 'agent-doc',
                name: 'editor_route',
                authority_generation: 0,
                idempotency_key: routeKey,
                deadline_ms: waitForReadySecs * 1000,
                policy: { dedupe: 'same_idempotency_key', supersede: false, cancel_on_preempt: true },
                payload_type: 'agent-doc.editor_route.v1',
                payload_hash: payloadHash,
                payload: { Inline: Array.from(payloadBytes) },
                required_features: ['causal-receipts', 'command-events'],
            },
        },
    };
}

// Resolve a unary `call` from the controller's returned command projection.
// Terminal-only: throws on a non-terminal projection or a non-`applied` terminal.
export function resolveEditorRouteTerminal(data: any, commandId: string): string {
    const commands = data?.projection?.commands;
    const entry = Array.isArray(commands)
        ? commands.find((c: any) => c?.command_id === commandId)
        : undefined;
    const output = typeof data?.output === 'string' ? data.output : '';
    if (!entry || entry.terminal !== true) {
        throw new Error(output || 'command plane returned a non-terminal projection for editor_route');
    }
    if (entry.status !== 'applied') {
        throw new Error(output || `editor_route ${entry.status}: ${entry.reason ?? 'rejected'}`);
    }
    return output;
}
