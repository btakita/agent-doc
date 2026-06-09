package com.github.btakita.agentdoc

/**
 * Pure decision for the phase-2 pre-emptive `/clear` before a Run Agent Doc
 * dispatch (`#s760` / review `#clear-opt-in-threshold`).
 *
 * Phase 1 (shipped, binary side): `agent_doc_clear_threshold` config resolved
 * via `session_accretion::clear_threshold_for_doc` and surfaced on preflight
 * `session_accretion.clear_threshold`.
 *
 * Phase 2 (this seam): when the live Claude Code context-usage percentage meets
 * or exceeds the configured threshold AND the operator opted into
 * `agent_doc_queue_context_reset`, the plugin should run session clear before
 * the next dispatch so the next turn starts with headroom.
 *
 * This object holds ONLY the decision so it is unit-testable without a live IDE.
 * The two genuinely live-gated inputs — obtaining the live context-usage % from
 * the Claude Code pane, and wiring the trigger into the dispatch path — are
 * tracked under `#xkpf` (live-verify batch). The decision fails safe: an unknown
 * percentage (`null`) or a disabled opt-in never clears.
 */
internal object ContextResetDecision {
    /**
     * @param contextUsagePct live Claude Code context-usage percent (0..100), or
     *   `null` when it could not be read (the gated pane-read source). `null`
     *   never triggers a clear.
     * @param clearThreshold preflight `session_accretion.clear_threshold`
     *   (0..100). A threshold of 0 disables the gate (never clears).
     * @param optIn the `agent_doc_queue_context_reset` opt-in.
     */
    fun shouldClearBeforeDispatch(contextUsagePct: Int?, clearThreshold: Int, optIn: Boolean): Boolean {
        if (!optIn) return false
        if (clearThreshold <= 0) return false
        val pct = contextUsagePct ?: return false
        return pct >= clearThreshold
    }
}
