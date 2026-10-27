export type SessionCommandName = 'status' | 'restart-supervisor' | 'restart-agent' | 'stop-agent' | 'cancel-turn' | 'clear' | 'interrupt-clear' | 'doctor';

export interface OutputPresentation {
    title: string;
    body: string;
}

export interface SessionStatusPresentation extends OutputPresentation {
    hint: string;
}

export interface RouteFailurePresentation extends OutputPresentation {
    toast: string;
}

/*
 * `#vscodesessionparsers`: what a Clear Session Context or Restart Supervisor
 * command's output means (cleared, queued, deferred, or refused clear; an
 * editor-holds-pane, busy, or starting restart refusal; their details and
 * message text) is the native `session_command_outcome` derivation, asked
 * through `deriveSessionCommandOutcome` in `sessionCommandOutcome.ts`. Never
 * parse that output here; change the native derivation and its shared parity
 * file instead.
 */

/** The Project Controller→plugin turn-state projection consumed by editor UI. */
export interface TurnProjection {
    state: 'idle' | 'awaiting_response' | 'persisting';
    turn_in_flight: boolean;
    transition_authority: string;
    /** One-line label of the in-flight task (`#turntasklabel`). */
    task?: string;
    semantic_merge_conflicts?: Array<{
        component: string;
        id: string;
        reason: string;
        detail: string;
    }>;
    /** `#clearsupersedenotice`: the latest queued clear the supervisor dropped unsent. */
    deferred_clear_superseded?: DeferredClearSuperseded;
    realtime_steering?: {
        observed_content_hash?: string;
        state?: 'prompt_target' | 'content_edit' | 'prompt_deleted' | 'prompt_reduced';
        count?: number;
        preview?: string;
        verbatim?: string;
        elements?: Record<string, {
            state: 'prompt_target' | 'content_edit' | 'prompt_deleted' | 'prompt_reduced';
            ordinal: number;
            preview?: string;
            verbatim: string;
        }>;
    };
}

/** `#clearsupersedenotice`: a queued Clear Session Context the supervisor dropped. */
export interface DeferredClearSuperseded {
    clear_epoch: number;
    pane: string;
    reason: string;
    superseded_at?: number;
}

/**
 * `#clearsupersedenotice`: announce each dropped queued clear once per `clear_epoch`. The first
 * ready projection observed for a document seeds the baseline (an old drop is not news), and a
 * projection without the field never resets it, so a reconnect cannot re-raise the same drop.
 * Parity with JetBrains `DeferredClearSupersededNotices`.
 */
export class DeferredClearSupersededNotices {
    private readonly lastEpoch = new Map<string, number>();

    observe(filePath: string, dropped: DeferredClearSuperseded | null | undefined): boolean {
        const seen = this.lastEpoch.get(filePath);
        if (seen === undefined) {
            this.lastEpoch.set(filePath, dropped ? dropped.clear_epoch : -1);
            return false;
        }
        if (dropped && dropped.clear_epoch !== seen) {
            this.lastEpoch.set(filePath, dropped.clear_epoch);
            return true;
        }
        return false;
    }

    forget(filePath: string): void {
        this.lastEpoch.delete(filePath);
    }
}

export function buildDeferredClearSupersededMessage(
    relativePath: string,
    dropped: DeferredClearSuperseded,
): string {
    const why = dropped.reason === 'newer_prompt'
        ? `A newer prompt started in pane ${dropped.pane} before the turn it was queued behind `
            + 'ended, so the clear was dropped to keep the context that prompt is using.'
        : `The supervisor dropped it in pane ${dropped.pane} (reason=${dropped.reason}).`;
    return `Queued Clear Session Context was not sent for ${relativePath}.\n`
        + `${why} Press Clear Session Context again if you still want a fresh context, `
        + 'or use Interrupt and clear to clear immediately.';
}

export interface TurnStatePresentation {
    /** Short status label reflecting the Project Controller turn phase ('' when idle). */
    label: string;
    /** True when a forwarded operator prompt would collide with an in-flight
     * response (the `live_prompt_drift` double-append guard): the plugin queues it
     * for the next turn instead of feeding it into the current exchange. */
    guardPromptForwarding: boolean;
    /** Full aggregate steering for status-bar hover text when present. */
    tooltip?: string;
}

type TurnSteeringState = NonNullable<TurnProjection['realtime_steering']>['state'];

/**
 * Consume the Project Controller's authoritative turn-state projection into a plugin-facing
 * status label + the double-append forwarding guard. This is how the plugin
 * COORDINATES with the turn phase (the Project Controller owns the phase,
 * the plugin observes this projection). Parity with the JetBrains frontend
 * (`specs/14-realtime-workflow.md` § Editor Parity Requirement).
 */
export function buildTurnStatePresentation(
    projection: TurnProjection | null,
): TurnStatePresentation {
    if (!projection) {
        return { label: '', guardPromptForwarding: false };
    }
    const conflicts = projection.semantic_merge_conflicts ?? [];
    const conflictLabel = conflicts.length > 0
        ? `⚠ merge conflict${conflicts.length === 1 ? '' : `s (${conflicts.length})`}`
        : null;
    const conflictTooltip = conflicts
        .map(conflict => `${conflict.component}:${conflict.id} — ${conflict.detail}`)
        .join('\n') || undefined;
    if (projection.state === 'idle' || !projection.turn_in_flight) {
        return {
            label: conflictLabel ? `agent-doc: ${conflictLabel}` : '',
            guardPromptForwarding: false,
            ...(conflictTooltip ? { tooltip: conflictTooltip } : {}),
        };
    }
    const phaseLabel =
        projection.state === 'awaiting_response'
            ? '⟳ agent-doc: awaiting response'
            : '⟳ agent-doc: persisting';
    const steeringCount = projection.realtime_steering?.count ?? 0;
    const steeringBase = steeringLabel(projection.realtime_steering?.state);
    const steering = steeringBase && steeringCount > 1
        ? `${steeringBase} (${steeringCount} edits)`
        : steeringBase;
    // `#turntasklabel`: which task is running, as one truncated line; full text in the tooltip.
    const task = projection.task?.replace(/\s+/g, ' ').trim() || null;
    const label = [phaseLabel, task ? truncateTaskLabel(task) : null, steering, conflictLabel]
        .filter(Boolean)
        .join(' · ');
    const tooltip = [
        task ? `Task: ${task}` : null,
        projection.realtime_steering?.verbatim?.trim(),
        conflictTooltip,
    ].filter(Boolean).join('\n\n') || undefined;
    return { label, guardPromptForwarding: true, ...(tooltip ? { tooltip } : {}) };
}

export const TURN_TASK_LABEL_MAX_CHARS = 60;

export function truncateTaskLabel(task: string, max = TURN_TASK_LABEL_MAX_CHARS): string {
    const chars = Array.from(task);
    return chars.length <= max ? task : `${chars.slice(0, max - 1).join('').trimEnd()}…`;
}

function steeringLabel(state: TurnSteeringState | undefined): string | null {
    switch (state) {
        case 'prompt_target':
            return 'new prompt';
        case 'content_edit':
            return 'document edited';
        case 'prompt_deleted':
            return 'prompt deleted';
        case 'prompt_reduced':
            return 'prompt reduced';
        default:
            return null;
    }
}

export function buildSessionCommandArgs(
    command: SessionCommandName,
    relativePath: string,
): string[] {
    return ['session', command, relativePath];
}

export function buildForcedRestartSupervisorCommandArgs(relativePath: string): string[] {
    return ['session', 'restart-supervisor', '--force', relativePath];
}

export function buildSessionStatusPresentation(
    relativePath: string,
    output: string,
): SessionStatusPresentation {
    return {
        title: `Session status: ${relativePath}`,
        body: normalizeOutputBody(output),
        hint: `Session status: ${relativePath}`,
    };
}

export function buildSessionSuccessHint(
    command: Exclude<SessionCommandName, 'status'>,
    relativePath: string,
    output: string,
): string {
    const trimmed = output.trim();
    if (trimmed) return trimmed;
    switch (command) {
    case 'restart-supervisor':
        return `Recycle requested for supervisor handling ${relativePath}`;
    case 'restart-agent':
        return `Restart requested for agent handling ${relativePath}`;
        case 'stop-agent':
            return `Stopped agent for ${relativePath} (supervisor still running)`;
        case 'cancel-turn':
            return `Cancelled turn for ${relativePath} (no-op if the agent was idle)`;
        case 'clear':
            return `Cleared session context for ${relativePath}`;
        case 'interrupt-clear':
            return `Interrupted and cleared session context for ${relativePath}`;
        case 'doctor':
            return `Copied session diagnostics for ${relativePath}`;
    }
}

export function buildRouteFailurePresentation(
    relativePath: string,
    output: string,
): RouteFailurePresentation {
    const trimmed = output.trim();
    const firstLine = trimmed.split('\n')[0]?.trim() || 'route failed';
    return {
        title: `Route failure: ${relativePath}`,
        body: normalizeOutputBody(trimmed),
        toast: `route failed: ${firstLine}`,
    };
}

export function sessionStatusShowsIdleDirectPane(output: string): boolean {
    return output.split(/\r?\n/).some((line) =>
        line.startsWith('live_pane:') &&
        line.includes('state=alive-idle') &&
        line.includes('prompt_ready=true'),
    );
}

function normalizeOutputBody(output: string): string {
    const trimmed = output.trim();
    return trimmed || '(no output)';
}
