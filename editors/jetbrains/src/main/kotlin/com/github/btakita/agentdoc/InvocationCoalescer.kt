package com.github.btakita.agentdoc

/**
 * Per-doc, per-action coalesce guard for editor-triggered agent-doc invocations
 * (`#9adk` / review `#console-input-accumulation`).
 *
 * The route/binary layer already gates dispatch on pane quiescence and dedups
 * pending dispatches into `agent:queue`. The residual gap is at the editor
 * terminal layer: a rapidly re-fired action (auto-loop tick racing a manual
 * click, or a double key-chord) could enqueue a second `/agent-doc` / `/clear`
 * keystroke before the first one's route even starts. This guard coalesces a
 * second invocation of the **same action** for the **same document** that
 * arrives within a short window, so duplicate console keystrokes never stack up.
 *
 * Keyed by `<actionKind>:<routeKey>` so a Run and a Clear for the same document
 * do NOT coalesce each other — only same-kind rapid re-fires collapse. The
 * decision is pure given `nowMillis`, so it is unit-testable without a live IDE.
 */
internal object InvocationCoalescer {
    /** Default coalesce window. Long enough to absorb an auto-loop/manual double
     *  fire, short enough not to swallow a deliberate repeat a moment later. */
    const val DEFAULT_WINDOW_MILLIS = 750L

    private val lastAccepted = mutableMapOf<String, Long>()

    /**
     * Returns true if this invocation should proceed, false if it should be
     * coalesced (an accepted invocation for the same key landed within
     * `windowMillis`). On `true`, records `nowMillis` as the latest accepted
     * invocation for the key.
     */
    fun shouldProceed(key: String, nowMillis: Long, windowMillis: Long = DEFAULT_WINDOW_MILLIS): Boolean {
        synchronized(lastAccepted) {
            val last = lastAccepted[key]
            if (last != null && nowMillis - last < windowMillis) {
                return false
            }
            lastAccepted[key] = nowMillis
            return true
        }
    }

    /** Compose the coalesce key for an action kind + route key. */
    fun key(actionKind: String, routeKey: String): String = "$actionKind:$routeKey"

    /** Test-only: reset accumulated state between cases. */
    internal fun resetForTest() {
        synchronized(lastAccepted) { lastAccepted.clear() }
    }
}
