/**
 * What a finished Clear Session Context or Restart Supervisor command's output
 * means for the operator (`#vscodesessionparsers`, `editors/SPEC.md`
 * § "Session command outcome").
 *
 * The extension runs `agent-doc session clear` and
 * `agent-doc session restart-supervisor [--force]` and gets back the output
 * and whether the command exited zero. Reading that output (whether a clear
 * ran now, was queued behind a live turn, a settling prompt or an accepted
 * run, was deferred under a busy auto-loop, or was refused; whether a restart
 * was refused because a terminal editor holds the pane, the pane is busy, or
 * the actor is still starting; the details of each; and the notification
 * text) is the native `session_command_outcome` derivation of
 * `agent_doc_editor_derive_json` (`agent-doc-editor-surface`
 * `session_command_outcome.rs`), the same derivation the JetBrains plugin
 * asks. The native library hot-reloads under a running extension host, so a
 * wording change in the binary needs no VSIX. The extension executes the
 * answer: the message type and its actions stay in `extension.ts`.
 *
 * {@link deriveSessionCommandOutcomeFloor} is the frozen TypeScript port of
 * the native rules (java.util.regex ASCII `\s`/`\S`/`\d`/`\b`, Kotlin
 * `trim`/`isBlank` whitespace, `toLongOrNull`, `substringBefore`,
 * `removeSuffix`, `removeSurrounding`), used only when the loaded library
 * cannot answer, cannot answer a kind this generation executes, or leaves out
 * a refusal's details. Both answer every case of the shared parity file
 * `editors/jetbrains/src/test/resources/editor-derivations/session_command_outcome.json`;
 * new rules land in the Rust derivation, never here.
 */

export const SESSION_COMMAND_OUTCOME_DERIVATION = 'session_command_outcome';

/** The session commands whose output this derivation reads. */
export type SessionOutcomeCommand = 'clear' | 'restart';

/** Wire shape of Rust `SessionCommandOutcomeEvidence`. */
export interface SessionCommandOutcomeEvidence {
    command: SessionOutcomeCommand;
    succeeded: boolean;
    force: boolean;
    /** The command's output, trimmed, exactly as the extension read it. */
    output: string;
    relative_path: string;
}

/** The outcome kinds this extension generation executes (wire names). */
export const SESSION_COMMAND_OUTCOME_KINDS = [
    'generic',
    'cleared',
    'clear_queued',
    'clear_deferred',
    'clear_refused',
    'restart_refused_editor_holds_pane',
    'restart_refused_busy',
    'restart_refused_starting',
] as const;

export type SessionCommandOutcomeKind = typeof SESSION_COMMAND_OUTCOME_KINDS[number];

export interface SessionClearQueued {
    file: string;
    pane: string;
    evidence: string;
    evidence_age_secs: number | null;
    rearmed: boolean;
    settles_in_secs: number | null;
    pre_turn_grace_secs: number | null;
}

export interface SessionClearRefused {
    file: string;
    pane: string;
    source: string;
    current_command: string;
    tail: string;
    /** Non-empty for a protected prompt input refusal. */
    protected_reason: string;
    /** Non-empty for a live turn no supervisor owns. */
    live_turn_evidence: string;
}

export interface SessionRestartRefusedBusy {
    file: string;
    pane: string;
    source: string;
    current_command: string;
    tail: string;
}

export interface SessionRestartRefusedStarting {
    file: string;
    reason: string;
}

export interface SessionRestartRefusedEditorHoldsPane {
    file: string;
    pane: string;
    editor: string;
    source: string;
    current_command: string;
    tail: string;
}

/**
 * Wire shape of Rust `SessionCommandOutcome`. `summary` is `null` exactly for
 * `generic`; the detail its kind names is set and every other detail is `null`.
 */
export interface SessionCommandOutcome {
    kind: SessionCommandOutcomeKind;
    summary: string | null;
    clear_queued: SessionClearQueued | null;
    clear_refused: SessionClearRefused | null;
    restart_refused_busy: SessionRestartRefusedBusy | null;
    restart_refused_starting: SessionRestartRefusedStarting | null;
    restart_refused_editor_holds_pane: SessionRestartRefusedEditorHoldsPane | null;
}

// ---------------------------------------------------------------------------
// Java / Kotlin text semantics
// ---------------------------------------------------------------------------

/** The members of `java.util.regex` `\s` (ASCII), as class contents. */
const JAVA_SPACE_MEMBERS = ' \\t\\n\\x0B\\x0C\\r';

/**
 * A `java.util.regex` pattern as a JavaScript one. JavaScript's `\s` is
 * Unicode-aware, Java's is ASCII; `\d` and `\b` (without the `u` flag) are
 * ASCII in both. JavaScript (without the `v` flag) has no nested classes, so
 * a `\s` inside a class becomes its members. `dotAll` stands for
 * `DOT_MATCHES_ALL` / `(?s)`.
 */
function javaRegex(pattern: string, dotAll = false): RegExp {
    let translated = '';
    let inClass = false;
    for (let index = 0; index < pattern.length; index += 1) {
        const char = pattern[index];
        if (char === '\\') {
            const next = pattern[index + 1];
            index += 1;
            if (next === 's') {
                translated += inClass ? JAVA_SPACE_MEMBERS : `[${JAVA_SPACE_MEMBERS}]`;
            } else if (next === 'S') {
                if (inClass) throw new Error(`\\S inside a class has no ASCII translation: ${pattern}`);
                translated += `[^${JAVA_SPACE_MEMBERS}]`;
            } else {
                translated += `\\${next}`;
            }
            continue;
        }
        if (char === '[' && !inClass) inClass = true;
        else if (char === ']' && inClass) inClass = false;
        translated += char;
    }
    return new RegExp(translated, dotAll ? 's' : '');
}

/**
 * Kotlin's `Char.isWhitespace` on the JVM: `Character.isWhitespace(c) ||
 * Character.isSpaceChar(c)`. Unlike JavaScript's `\s` it excludes U+0085 and
 * U+FEFF and includes U+001C..U+001F.
 */
function kotlinIsWhitespace(code: number): boolean {
    return (code >= 0x09 && code <= 0x0D)
        || (code >= 0x1C && code <= 0x20)
        || code === 0xA0
        || code === 0x1680
        || (code >= 0x2000 && code <= 0x200A)
        || code === 0x2028
        || code === 0x2029
        || code === 0x202F
        || code === 0x205F
        || code === 0x3000;
}

/** Kotlin `String.trim()`. */
function kotlinTrim(value: string): string {
    let start = 0;
    let end = value.length;
    while (start < end && kotlinIsWhitespace(value.charCodeAt(start))) start += 1;
    while (end > start && kotlinIsWhitespace(value.charCodeAt(end - 1))) end -= 1;
    return value.slice(start, end);
}

/** Kotlin `CharSequence.isBlank()`. */
function kotlinIsBlank(value: string): boolean {
    for (let index = 0; index < value.length; index += 1) {
        if (!kotlinIsWhitespace(value.charCodeAt(index))) return false;
    }
    return true;
}

/** Kotlin `ifBlank { fallback }`. */
function ifBlank(value: string, fallback: string): string {
    return kotlinIsBlank(value) ? fallback : value;
}

/** Kotlin `substringBefore(delimiter)`: the whole string when it is absent. */
function substringBefore(value: string, delimiter: string): string {
    const index = value.indexOf(delimiter);
    return index < 0 ? value : value.slice(0, index);
}

/** Kotlin `removeSuffix(suffix)`. */
function removeSuffix(value: string, suffix: string): string {
    return value.endsWith(suffix) ? value.slice(0, value.length - suffix.length) : value;
}

/** Kotlin `removeSurrounding(delimiter)`. */
function removeSurrounding(value: string, delimiter: string): string {
    return value.length >= delimiter.length * 2 && value.startsWith(delimiter) && value.endsWith(delimiter)
        ? value.slice(delimiter.length, value.length - delimiter.length)
        : value;
}

const LONG_MAX = BigInt('9223372036854775807');

/** Kotlin `toLongOrNull()` of a `\d+` capture. */
function toLongOrNull(digits: string | undefined): number | null {
    if (digits === undefined || !/^[0-9]+$/.test(digits)) return null;
    return BigInt(digits) > LONG_MAX ? null : Number(digits);
}

/** The first capture group of `regex`'s first match in `text`, or empty. */
function groupOne(regex: RegExp, text: string): string {
    return regex.exec(text)?.[1] ?? '';
}

// ---------------------------------------------------------------------------
// The frozen rules (a port of `session_command_outcome.rs`)
// ---------------------------------------------------------------------------

const PENDING_RUN_CLEAR_EVIDENCE = 'dispatch_turn_pending';
const DEFERRED_QUEUE_PREEMPT_CLEAR_MARKER = 'session_clear deferred for';

const BUSY_CLEAR_REFUSAL_HEADER = javaRegex(
    'session_clear refused for (.+?) because pane (\\S+) is (?:alive-busy|active_agent_doc|busy)',
    true,
);
const LIVE_TURN_CLEAR_REFUSAL_HEADER = javaRegex(
    'session_clear refused for (.+?) because a turn is live in pane (\\S+) \\(harness_turn_live=true evidence=([^,\\s)]+)',
    true,
);
const LIVE_TURN_CLEAR_QUEUED = javaRegex(
    'session_clear queued for (.+?) — (?:(?:a turn is live|the last prompt) in|Run Agent Doc for) pane (\\S+)',
    true,
);
const LIVE_TURN_CLEAR_QUEUED_SETTLES_IN = javaRegex('\\bsettles_in_secs=(\\d+)');
const LIVE_TURN_CLEAR_QUEUED_PRE_TURN_GRACE = javaRegex('\\bpre_turn_grace_secs=(\\d+)');
const LIVE_TURN_CLEAR_QUEUED_EVIDENCE = javaRegex('\\bevidence=([^,\\s)]+)');
const LIVE_TURN_CLEAR_QUEUED_AGE = javaRegex('\\bevidence_age_secs=(\\d+)');
const LIVE_TURN_CLEAR_QUEUED_REARMED = javaRegex('\\brearmed=(true|false)\\b');
const PROTECTED_CLEAR_REFUSAL_HEADER = javaRegex(
    'session_clear refused for (.+?) because pane (\\S+) contains protected prompt input',
    true,
);
const BUSY_RESTART_REFUSAL_HEADER = javaRegex(
    'session_restart refused for (.+?) because pane (\\S+) is alive-busy',
    true,
);
const STARTING_RESTART_REFUSAL = javaRegex(
    'session_restart refused for (.+?) because the authoritative actor is still starting and (.+?)\\. Wait for a dispatch-ready prompt',
    true,
);
const EDITOR_RESTART_REFUSAL_HEADER = javaRegex(
    'session_restart refused for (.+?) because pane (\\S+) is held by editor (\\S+)',
    true,
);
const PROTECTED_CLEAR_REASON = javaRegex('reason=([^,)]+)');
const BUSY_CLEAR_SOURCE = javaRegex('source=([^,)]+)');
const BUSY_CLEAR_COMMAND = javaRegex('current_command=([^,)]+)');

/** The output after `match`, the text its detail fields are read from. */
function detailAfter(output: string, match: RegExpExecArray): string {
    return output.slice(match.index + match[0].length);
}

function extractBusyClearTail(detail: string): string {
    const marker = 'tail=';
    const start = detail.indexOf(marker);
    if (start < 0) return '';
    let raw = detail.slice(start + marker.length);
    for (const delimiter of [
        '). Run `agent-doc session status',
        '). Run agent-doc session status',
        '). Clear the prompt input manually',
        '). Wait for an idle prompt',
    ]) {
        raw = substringBefore(raw, delimiter);
    }
    raw = kotlinTrim(raw);
    raw = removeSuffix(raw, ').');
    raw = removeSuffix(raw, ')');
    raw = kotlinTrim(raw);
    return removeSurrounding(raw, '"').split('\\"').join('"').split('\\\\').join('\\');
}

function parseClearQueued(output: string): SessionClearQueued | null {
    const match = LIVE_TURN_CLEAR_QUEUED.exec(output);
    if (!match) return null;
    const detail = detailAfter(output, match);
    return {
        file: match[1],
        pane: match[2],
        evidence: groupOne(LIVE_TURN_CLEAR_QUEUED_EVIDENCE, detail),
        evidence_age_secs: toLongOrNull(LIVE_TURN_CLEAR_QUEUED_AGE.exec(detail)?.[1]),
        rearmed: LIVE_TURN_CLEAR_QUEUED_REARMED.exec(detail)?.[1] === 'true',
        settles_in_secs: toLongOrNull(LIVE_TURN_CLEAR_QUEUED_SETTLES_IN.exec(detail)?.[1]),
        pre_turn_grace_secs: toLongOrNull(LIVE_TURN_CLEAR_QUEUED_PRE_TURN_GRACE.exec(detail)?.[1]),
    };
}

function parseClearRefused(output: string): SessionClearRefused | null {
    const liveTurn = LIVE_TURN_CLEAR_REFUSAL_HEADER.exec(output);
    if (liveTurn) {
        const detail = detailAfter(output, liveTurn);
        return {
            file: liveTurn[1],
            pane: liveTurn[2],
            source: ifBlank(groupOne(BUSY_CLEAR_SOURCE, detail), 'unknown'),
            current_command: 'unknown',
            tail: 'unknown',
            protected_reason: '',
            live_turn_evidence: liveTurn[3],
        };
    }
    const protectedInput = PROTECTED_CLEAR_REFUSAL_HEADER.exec(output);
    if (protectedInput) {
        const detail = detailAfter(output, protectedInput);
        return {
            file: protectedInput[1],
            pane: protectedInput[2],
            source: ifBlank(groupOne(BUSY_CLEAR_SOURCE, detail), 'unknown'),
            current_command: ifBlank(groupOne(BUSY_CLEAR_COMMAND, detail), 'unknown'),
            tail: extractBusyClearTail(detail),
            protected_reason: ifBlank(groupOne(PROTECTED_CLEAR_REASON, detail), 'protected prompt input'),
            live_turn_evidence: '',
        };
    }
    const busy = BUSY_CLEAR_REFUSAL_HEADER.exec(output);
    if (!busy) return null;
    const detail = detailAfter(output, busy);
    return {
        file: busy[1],
        pane: busy[2],
        source: ifBlank(groupOne(BUSY_CLEAR_SOURCE, detail), 'unknown'),
        current_command: ifBlank(groupOne(BUSY_CLEAR_COMMAND, detail), 'unknown'),
        tail: extractBusyClearTail(detail),
        protected_reason: '',
        live_turn_evidence: '',
    };
}

function parseRestartRefusedBusy(output: string): SessionRestartRefusedBusy | null {
    const match = BUSY_RESTART_REFUSAL_HEADER.exec(output);
    if (!match) return null;
    const detail = detailAfter(output, match);
    return {
        file: match[1],
        pane: match[2],
        source: ifBlank(groupOne(BUSY_CLEAR_SOURCE, detail), 'unknown'),
        current_command: ifBlank(groupOne(BUSY_CLEAR_COMMAND, detail), 'unknown'),
        tail: extractBusyClearTail(detail),
    };
}

function parseRestartRefusedStarting(output: string): SessionRestartRefusedStarting | null {
    const match = STARTING_RESTART_REFUSAL.exec(output);
    if (!match) return null;
    return { file: match[1], reason: kotlinTrim(match[2]) };
}

function parseRestartRefusedEditorHoldsPane(output: string): SessionRestartRefusedEditorHoldsPane | null {
    const match = EDITOR_RESTART_REFUSAL_HEADER.exec(output);
    if (!match) return null;
    const detail = detailAfter(output, match);
    return {
        file: match[1],
        pane: match[2],
        editor: match[3],
        source: ifBlank(groupOne(BUSY_CLEAR_SOURCE, detail), 'unknown'),
        current_command: ifBlank(groupOne(BUSY_CLEAR_COMMAND, detail), 'unknown'),
        tail: extractBusyClearTail(detail),
    };
}

/** Kotlin `formatEvidenceAge`. */
function formatEvidenceAge(secs: number): string {
    if (secs < 120) return `${secs}s`;
    if (secs < 7200) return `${Math.trunc(secs / 60)}m ${secs % 60}s`;
    return `${Math.trunc(secs / 3600)}h ${Math.trunc((secs % 3600) / 60)}m`;
}

function clearQueuedMessage(relativePath: string, queued: SessionClearQueued): string {
    if (queued.settles_in_secs !== null) {
        return cancelledPromptClearQueuedMessage(relativePath, queued, queued.settles_in_secs);
    }
    if (queued.evidence === PENDING_RUN_CLEAR_EVIDENCE) {
        return pendingRunClearQueuedMessage(relativePath, queued);
    }
    let message = queued.rearmed
        ? `Clear Session Context re-armed for ${relativePath}: still one clear, it runs when the turn in pane `
        : `Clear Session Context queued for ${relativePath}: it runs when the turn in pane `;
    message += `${queued.pane} ends.\nNothing was typed into the running turn`;
    if (!kotlinIsBlank(queued.evidence)) {
        message += ` (live-turn evidence: ${queued.evidence}`;
        if (queued.evidence_age_secs !== null) {
            message += `, ${formatEvidenceAge(queued.evidence_age_secs)} old`;
        }
        message += ')';
    }
    message += '. The clear is dropped if a new prompt starts first. ';
    message += 'Use Interrupt and clear to clear immediately instead.';
    return message;
}

function cancelledPromptClearQueuedMessage(
    relativePath: string,
    queued: SessionClearQueued,
    settlesInSecs: number,
): string {
    return (queued.rearmed ? 'Clear Session Context is already queued for ' : 'Clear Session Context queued for ')
        + `${relativePath}: it runs automatically in about ${formatEvidenceAge(settlesInSecs)}.\n`
        + `The last prompt in pane ${queued.pane}`
        + ' never started a turn (it looks cancelled); agent-doc waits for that to be confirmed before '
        + 'typing /clear. Pressing Clear again does not make it sooner. '
        + 'Use Interrupt and clear to clear immediately instead.';
}

function pendingRunClearQueuedMessage(relativePath: string, queued: SessionClearQueued): string {
    let message = (queued.rearmed ? 'Clear Session Context is already queued for ' : 'Clear Session Context queued for ')
        + `${relativePath}: it runs after the Run Agent Doc `;
    message += queued.evidence_age_secs !== null
        ? `accepted ${formatEvidenceAge(queued.evidence_age_secs)} ago`
        : 'already accepted';
    message += ' finishes.\n';
    message += `That run has not started its turn in pane ${queued.pane}`;
    message += ' yet, so nothing is running and nothing was typed; a clear pressed after Run Agent Doc runs after that run, ';
    message += 'and that run does not cancel it.';
    if (queued.pre_turn_grace_secs !== null) {
        const remaining = Math.max(queued.pre_turn_grace_secs - (queued.evidence_age_secs ?? 0), 0);
        message += ` If the run never starts, the clear runs within about ${formatEvidenceAge(remaining)}.`;
    }
    message += ' Pressing Clear again or Interrupt and clear does not make it sooner.';
    return message;
}

function clearDeferredMessage(relativePath: string): string {
    return `Clear Session Context deferred for ${relativePath}.\n`
        + 'The pane is busy under an active queue auto-loop, so the loop was paused and the clear will run '
        + 'automatically at the next idle gap, then the loop resumes. Use Interrupt and clear to clear immediately instead.';
}

function shownValue(value: string): string | null {
    return !kotlinIsBlank(value) && value !== 'unknown' ? value : null;
}

function latestPaneOutput(tail: string): string {
    const shown = shownValue(tail);
    return shown === null ? '' : `\nLatest pane output: ${shown}`;
}

function clearRefusedMessage(relativePath: string, refusal: SessionClearRefused): string {
    if (!kotlinIsBlank(refusal.protected_reason)) {
        return `Clear Session Context is blocked for ${relativePath}.\nPane ${refusal.pane}`
            + ` contains protected prompt input (${refusal.protected_reason}). `
            + 'Use Interrupt and clear to discard the prompt input, or Show status to inspect the session.'
            + latestPaneOutput(refusal.tail);
    }
    if (!kotlinIsBlank(refusal.live_turn_evidence)) {
        return `Clear Session Context was not queued for ${relativePath}.\nA turn is running in pane ${refusal.pane}`
            + ` (${refusal.live_turn_evidence}), and no agent-doc supervisor owns that pane to run the clear when the turn ends.`
            + ' Nothing was sent. Retry Clear Session Context after the turn finishes, or use Interrupt and clear to discard the running turn.';
    }
    const command = shownValue(refusal.current_command);
    return `Session is still running for ${relativePath}.\nPane ${refusal.pane} is busy`
        + (command === null ? '' : ` (${command})`)
        + '. Wait for the turn to finish, then retry Clear Session Context.'
        + ' Use Refresh and retry if the pane has returned to an idle prompt, or Interrupt and clear to discard the running turn.'
        + latestPaneOutput(refusal.tail);
}

function restartRefusedBusyMessage(relativePath: string, refusal: SessionRestartRefusedBusy): string {
    const command = shownValue(refusal.current_command);
    return `Restart Supervisor is blocked for ${relativePath}.\nPane ${refusal.pane} is busy`
        + (command === null ? '' : ` (${command})`)
        + '. Use Interrupt and restart to stop the running turn and restart the supervisor, or Show status to inspect the session.'
        + latestPaneOutput(refusal.tail);
}

function restartRefusedStartingMessage(relativePath: string, refusal: SessionRestartRefusedStarting): string {
    return `Restart Supervisor is blocked for ${relativePath}.\nThe authoritative actor is still starting`
        + (kotlinIsBlank(refusal.reason) ? '' : ` and ${refusal.reason}`)
        + '. Use Interrupt and restart to stop the current supervisor generation and restart anyway, or Show status to inspect the session.';
}

function restartRefusedEditorHoldsPaneMessage(
    relativePath: string,
    refusal: SessionRestartRefusedEditorHoldsPane,
): string {
    const editor = shownValue(kotlinIsBlank(refusal.editor) ? refusal.current_command : refusal.editor);
    return `Restart Supervisor is blocked for ${relativePath}.\nA terminal editor`
        + (editor === null ? '' : ` (${editor})`)
        + ` holds pane ${refusal.pane}`
        + ' — e.g. Claude Code `ctrl+g` edit-in-nvim. Close the editor (for example `:wq` in nvim) and retry;'
        + ' Interrupt and restart will not bypass it. Use Show status to inspect the session.'
        + latestPaneOutput(refusal.tail);
}

function generic(): SessionCommandOutcome {
    return {
        kind: 'generic',
        summary: null,
        clear_queued: null,
        clear_refused: null,
        restart_refused_busy: null,
        restart_refused_starting: null,
        restart_refused_editor_holds_pane: null,
    };
}

function withKind(
    kind: SessionCommandOutcomeKind,
    summary: string,
    detail: Partial<SessionCommandOutcome> = {},
): SessionCommandOutcome {
    return { ...generic(), kind, summary, ...detail };
}

function clearOutcome(output: string, relativePath: string, succeeded: boolean): SessionCommandOutcome {
    if (succeeded) {
        const queued = parseClearQueued(output);
        if (queued) {
            return withKind('clear_queued', clearQueuedMessage(relativePath, queued), { clear_queued: queued });
        }
        if (output.includes(DEFERRED_QUEUE_PREEMPT_CLEAR_MARKER)) {
            return withKind('clear_deferred', clearDeferredMessage(relativePath));
        }
        return withKind('cleared', kotlinIsBlank(output) ? `Cleared session context for ${relativePath}` : output);
    }
    const refusal = parseClearRefused(output);
    return refusal
        ? withKind('clear_refused', clearRefusedMessage(relativePath, refusal), { clear_refused: refusal })
        : generic();
}

function restartOutcome(
    output: string,
    relativePath: string,
    succeeded: boolean,
    force: boolean,
): SessionCommandOutcome {
    if (succeeded) return generic();
    // An editor holding the pane refuses Interrupt and restart too.
    const editor = parseRestartRefusedEditorHoldsPane(output);
    if (editor) {
        return withKind(
            'restart_refused_editor_holds_pane',
            restartRefusedEditorHoldsPaneMessage(relativePath, editor),
            { restart_refused_editor_holds_pane: editor },
        );
    }
    if (force) return generic();
    const busy = parseRestartRefusedBusy(output);
    if (busy) {
        return withKind('restart_refused_busy', restartRefusedBusyMessage(relativePath, busy), {
            restart_refused_busy: busy,
        });
    }
    const starting = parseRestartRefusedStarting(output);
    if (starting) {
        return withKind('restart_refused_starting', restartRefusedStartingMessage(relativePath, starting), {
            restart_refused_starting: starting,
        });
    }
    return generic();
}

/**
 * Compatibility floor: the frozen TypeScript port of the native rules. Never
 * extended; change `session_command_outcome.rs` and the parity file instead.
 */
export function deriveSessionCommandOutcomeFloor(
    evidence: Partial<SessionCommandOutcomeEvidence> & { command?: string },
): SessionCommandOutcome {
    const output = evidence.output ?? '';
    const relativePath = evidence.relative_path ?? '';
    const succeeded = evidence.succeeded ?? false;
    switch (evidence.command) {
    case 'clear':
        return clearOutcome(output, relativePath, succeeded);
    case 'restart':
        return restartOutcome(output, relativePath, succeeded, evidence.force ?? false);
    default:
        return generic();
    }
}

// ---------------------------------------------------------------------------
// The native answer
// ---------------------------------------------------------------------------

function record(value: unknown): Record<string, unknown> | null {
    return value !== null && typeof value === 'object' && !Array.isArray(value)
        ? value as Record<string, unknown>
        : null;
}

function text(value: Record<string, unknown>, name: string): string {
    return typeof value[name] === 'string' ? value[name] as string : '';
}

function flag(value: Record<string, unknown>, name: string): boolean {
    return value[name] === true;
}

function long(value: Record<string, unknown>, name: string): number | null {
    return typeof value[name] === 'number' && Number.isFinite(value[name]) ? value[name] as number : null;
}

function readClearQueued(raw: unknown): SessionClearQueued | null {
    const value = record(raw);
    if (!value) return null;
    return {
        file: text(value, 'file'),
        pane: text(value, 'pane'),
        evidence: text(value, 'evidence'),
        evidence_age_secs: long(value, 'evidence_age_secs'),
        rearmed: flag(value, 'rearmed'),
        settles_in_secs: long(value, 'settles_in_secs'),
        pre_turn_grace_secs: long(value, 'pre_turn_grace_secs'),
    };
}

function readClearRefused(raw: unknown): SessionClearRefused | null {
    const value = record(raw);
    if (!value) return null;
    return {
        file: text(value, 'file'),
        pane: text(value, 'pane'),
        source: text(value, 'source'),
        current_command: text(value, 'current_command'),
        tail: text(value, 'tail'),
        protected_reason: text(value, 'protected_reason'),
        live_turn_evidence: text(value, 'live_turn_evidence'),
    };
}

function readRestartRefusedBusy(raw: unknown): SessionRestartRefusedBusy | null {
    const value = record(raw);
    if (!value) return null;
    return {
        file: text(value, 'file'),
        pane: text(value, 'pane'),
        source: text(value, 'source'),
        current_command: text(value, 'current_command'),
        tail: text(value, 'tail'),
    };
}

function readRestartRefusedStarting(raw: unknown): SessionRestartRefusedStarting | null {
    const value = record(raw);
    return value ? { file: text(value, 'file'), reason: text(value, 'reason') } : null;
}

function readRestartRefusedEditorHoldsPane(raw: unknown): SessionRestartRefusedEditorHoldsPane | null {
    const value = record(raw);
    if (!value) return null;
    return {
        file: text(value, 'file'),
        pane: text(value, 'pane'),
        editor: text(value, 'editor'),
        source: text(value, 'source'),
        current_command: text(value, 'current_command'),
        tail: text(value, 'tail'),
    };
}

/** A refusal or queued kind is executable only with its details; the others need none. */
function executable(outcome: SessionCommandOutcome): boolean {
    switch (outcome.kind) {
    case 'clear_queued':
        return outcome.clear_queued !== null;
    case 'clear_refused':
        return outcome.clear_refused !== null;
    case 'restart_refused_editor_holds_pane':
        return outcome.restart_refused_editor_holds_pane !== null;
    case 'restart_refused_busy':
        return outcome.restart_refused_busy !== null;
    case 'restart_refused_starting':
        return outcome.restart_refused_starting !== null;
    default:
        return true;
    }
}

/**
 * Read a native `output`; `null` takes the floor. A kind this generation
 * cannot execute, or a kind whose details are missing, takes the whole floor;
 * a summary the answer leaves out (an older library) takes the floor's.
 */
export function parseNativeSessionCommandOutcome(
    raw: unknown,
    floor: SessionCommandOutcome,
): SessionCommandOutcome | null {
    const value = record(raw);
    if (!value) return null;
    const kind = SESSION_COMMAND_OUTCOME_KINDS.find((known) => known === value.kind);
    if (!kind) return null;
    const outcome: SessionCommandOutcome = {
        kind,
        summary: 'summary' in value
            ? (typeof value.summary === 'string' ? value.summary : null)
            : floor.summary,
        clear_queued: readClearQueued(value.clear_queued),
        clear_refused: readClearRefused(value.clear_refused),
        restart_refused_busy: readRestartRefusedBusy(value.restart_refused_busy),
        restart_refused_starting: readRestartRefusedStarting(value.restart_refused_starting),
        restart_refused_editor_holds_pane: readRestartRefusedEditorHoldsPane(value.restart_refused_editor_holds_pane),
    };
    return executable(outcome) ? outcome : null;
}

/**
 * Ask the native derivation what `evidence` means, falling back to the frozen
 * floor when `deriveNative` is absent, throws, or gives no readable answer.
 */
export function deriveSessionCommandOutcome(
    evidence: SessionCommandOutcomeEvidence,
    deriveNative?: (requestJson: string) => string | null,
): { outcome: SessionCommandOutcome; native: boolean } {
    const floor = deriveSessionCommandOutcomeFloor(evidence);
    if (deriveNative) {
        try {
            const raw = deriveNative(JSON.stringify({ derivation: SESSION_COMMAND_OUTCOME_DERIVATION, input: evidence }));
            if (raw) {
                const answer = JSON.parse(raw);
                if (answer?.derivation === SESSION_COMMAND_OUTCOME_DERIVATION) {
                    const outcome = parseNativeSessionCommandOutcome(answer.output, floor);
                    if (outcome) return { outcome, native: true };
                }
            }
        } catch {
            // fall through to the frozen floor
        }
    }
    return { outcome: floor, native: false };
}

/**
 * The combined output the derivation reads for a finished command, as the
 * JetBrains plugin reads it (`redirectErrorStream`): the binary reports a
 * queued or deferred clear on stderr while exiting zero, so stdout alone would
 * read every queued clear as one that ran now.
 */
export function combinedCommandOutput(stdout: string, stderr: string): string {
    return [stdout.trim(), stderr.trim()].filter((part) => part.length > 0).join('\n');
}
