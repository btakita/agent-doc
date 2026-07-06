package com.github.btakita.agentdoc

import com.google.gson.JsonParser
import com.intellij.openapi.diagnostic.Logger
import java.util.concurrent.TimeUnit

/**
 * Consumes the CPC's turn-state projection (the `agent_doc_turn_projection` FFI
 * export) into a plugin-facing status label plus the double-append forwarding
 * guard. This is how the JetBrains frontend COORDINATES with the CPC's
 * authoritative turn phase: the CPC owns the phase, the plugin observes this
 * projection. Parity with the VS Code `buildTurnStatePresentation`
 * (`specs/14-realtime-workflow.md` § Editor Parity Requirement).
 */
object TurnStateBridge {
    private val LOG = Logger.getInstance(TurnStateBridge::class.java)

    data class TurnStatePresentation(
        /** Short status label reflecting the CPC turn phase (empty when idle). */
        val label: String,
        /**
         * True when forwarding an operator prompt now would collide with an
         * in-flight response (the `live_prompt_drift` double-append guard): the
         * plugin queues it for the next turn instead of the current exchange.
         */
        val guardPromptForwarding: Boolean,
    )

    /** Raw turn-projection JSON for a document, or null when unavailable. */
    fun turnProjectionJsonForFile(filePath: String): String? {
        val started = System.nanoTime()
        val lib = AgentDocLib.get() ?: return null
        val ptr = try {
            lib.agent_doc_turn_projection(filePath)
        } catch (e: Throwable) {
            LOG.debug("[turn-projection] unavailable: ${e.message}")
            logProjectionTiming(filePath, started)
            return null
        }
        try {
            val raw = ptr?.getString(0) ?: return null
            val projection = raw.takeUnless { it == "null" }
            LOG.debug("[turn-projection] $filePath => ${projection ?: "null"}")
            return projection
        } finally {
            lib.agent_doc_free_string(ptr)
            logProjectionTiming(filePath, started)
        }
    }

    fun presentationForFile(filePath: String): TurnStatePresentation =
        turnProjectionJsonForFile(filePath)?.let(::presentation)
            ?: TurnStatePresentation("", false)

    /** Pure projection→presentation mapping (mirrors VS Code buildTurnStatePresentation). */
    fun presentation(json: String): TurnStatePresentation = try {
        val root = JsonParser.parseString(json).asJsonObject
        val state = root.get("state")?.asString ?: "idle"
        val inFlight = root.get("turn_in_flight")?.asBoolean ?: false
        if (state == "idle" || !inFlight) {
            TurnStatePresentation("", false)
        } else {
            val phaseLabel =
                if (state == "awaiting_response") "⟳ agent-doc: awaiting response"
                else "⟳ agent-doc: persisting"
            val steeringLabel = root.getAsJsonObject("realtime_steering")
                ?.get("state")
                ?.asString
                ?.let(::steeringLabel)
            val label = listOfNotNull(phaseLabel, steeringLabel).joinToString(" · ")
            TurnStatePresentation(label, true)
        }
    } catch (e: Throwable) {
        LOG.debug("[turn-projection] parse failed: ${e.message}")
        TurnStatePresentation("", false)
    }

    private fun steeringLabel(state: String): String? = when (state) {
        "prompt_target" -> "new prompt"
        "content_edit" -> "document edited"
        "prompt_deleted" -> "prompt deleted"
        "prompt_reduced" -> "prompt reduced"
        else -> null
    }

    private fun logProjectionTiming(filePath: String, startedNanos: Long) {
        val elapsedMs = TimeUnit.NANOSECONDS.toMillis(System.nanoTime() - startedNanos)
        val message = "[turn-perf] projection file=${java.io.File(filePath).name} elapsed_ms=$elapsedMs thread=${Thread.currentThread().name}"
        if (elapsedMs >= 100) {
            LOG.warn(message)
        } else {
            LOG.debug(message)
        }
    }
}
