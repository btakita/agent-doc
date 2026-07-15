package com.github.btakita.agentdoc

import com.google.gson.JsonParser
import com.intellij.openapi.diagnostic.Logger
import java.io.File
import java.util.concurrent.TimeUnit

/**
 * Consumes the Project Controller lazily state projection into a plugin-facing
 * status label plus the double-append forwarding guard. The Project Controller
 * owns the phase; the plugin observes the lazily mirror and never reads cycle
 * sidecars on the editor hot path.
 */
object TurnStateBridge {
    private val LOG = Logger.getInstance(TurnStateBridge::class.java)
    private val START_SESSION_PANE_ALIAS_REGEX = Regex(
        """start_session cross-document actor pane alias: pane (\S+) is already claimed by .*?\bgeneration=(\d+)\s+state=(\S+)""",
        RegexOption.DOT_MATCHES_ALL,
    )
    private val PROJECT_CONTROLLER_COMMAND_FAILED_REGEX = Regex(
        """project controller command\s+`?([A-Za-z0-9_-]+)`?\s+failed:\s*(.*)""",
        RegexOption.DOT_MATCHES_ALL,
    )

    data class TurnStatePresentation(
        /** Short status label reflecting the Project Controller turn phase (empty when idle). */
        val label: String,
        /**
         * True when forwarding an operator prompt now would collide with an
         * in-flight response (the `live_prompt_drift` double-append guard): the
         * plugin queues it for the next turn instead of the current exchange.
         */
        val guardPromptForwarding: Boolean,
        /** Details for status-bar hover text when the short label is lossy. */
        val tooltip: String? = null,
        /** Route-failure labels belong in the status bar, not the active-turn banner. */
        val showBanner: Boolean = true,
    )

    private data class ProjectControllerTurnRead(
        val projectionJson: String?,
        val connected: Boolean,
        val error: String? = null,
    )

    /** Raw Project Controller turn-projection JSON for a document, or null when unavailable. */
    fun turnProjectionJsonForFile(filePath: String): String? =
        turnProjectionReadForFile(filePath).projectionJson

    private fun turnProjectionReadForFile(filePath: String): ProjectControllerTurnRead {
        val started = System.nanoTime()
        val file = File(filePath)
        val root = findAgentDocProjectRoot(file)
            ?: return ProjectControllerTurnRead(
                projectionJson = null,
                connected = false,
                error = "No .agent-doc project root found for ${file.name}",
            ).also { logProjectionTiming(filePath, started) }
        val projection = try {
            StateProjectionBridge.subscribeMirrorForFileViaProjectController(filePath, root)
            StateProjectionBridge.mirrorTurnProjectionJsonForFile(filePath)
        } catch (e: Throwable) {
            LOG.debug("[turn-projection] Project Controller unavailable: ${e.message}")
            return ProjectControllerTurnRead(
                projectionJson = null,
                connected = false,
                error = e.message ?: "Project Controller request failed",
            ).also { logProjectionTiming(filePath, started) }
        }
        LOG.debug("[turn-projection] $filePath => ${projection ?: "null"}")
        logProjectionTiming(filePath, started)
        return ProjectControllerTurnRead(
            projectionJson = projection,
            connected = true,
        )
    }

    fun presentationForFile(filePath: String): TurnStatePresentation {
        val read = turnProjectionReadForFile(filePath)
        if (!read.connected) {
            return routeFailurePresentationForFile(filePath)
                ?: TurnStatePresentation(
                    label = "agent-doc: Project Controller disconnected",
                    guardPromptForwarding = false,
                    tooltip = "Agent Doc Project Controller is not connected for this document.\n${read.error ?: "Start or reconnect the Project Controller."}",
                    showBanner = false,
                )
        }
        val turnState = read.projectionJson?.let(::presentation)
        if (turnState != null && turnState.label.isNotEmpty()) {
            return turnState
        }
        return routeFailurePresentationForFile(filePath)
            ?: turnState
            ?: TurnStatePresentation("", false, showBanner = false)
    }

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
            val steering = root.getAsJsonObject("realtime_steering")
            val steeringCount = steering?.get("count")?.asInt ?: 0
            val steeringLabel = steering
                ?.get("state")
                ?.asString
                ?.let(::steeringLabel)
                ?.let { label -> if (steeringCount > 1) "$label ($steeringCount edits)" else label }
            val label = listOfNotNull(phaseLabel, steeringLabel).joinToString(" · ")
            val tooltip = steering?.get("verbatim")?.asString?.takeIf(String::isNotBlank)
            TurnStatePresentation(label, true, tooltip)
        }
    } catch (e: Throwable) {
        LOG.debug("[turn-projection] parse failed: ${e.message}")
        TurnStatePresentation("", false, showBanner = false)
    }

    internal fun routeFailurePresentationForFile(filePath: String): TurnStatePresentation? {
        val diagnostics = routeFailureDiagnosticsFileForFile(filePath) ?: return null
        val output = try {
            diagnostics.takeIf { it.isFile }?.readText()
        } catch (e: Exception) {
            LOG.debug("[route-failure-status] failed to read ${diagnostics.path}: ${e.message}")
            null
        } ?: return null
        return routeFailurePresentation(output)
    }

    internal fun routeFailurePresentation(routeOutput: String): TurnStatePresentation? {
        val reason = routeFailureReason(routeOutput) ?: return null
        val label = "⚠ agent-doc: ${routeFailureStatusSummary(reason)}"
        val tooltip = buildString {
            append("Agent Doc route failed.\nReason: ")
            append(reason)
            val fullOutput = routeOutput.trim()
            if (fullOutput.isNotEmpty() && fullOutput != reason) {
                append("\n\nFull route output:\n")
                append(fullOutput)
            }
        }
        return TurnStatePresentation(
            label = label,
            guardPromptForwarding = false,
            tooltip = tooltip,
            showBanner = false,
        )
    }

    internal fun routeFailureStatusSummary(reason: String): String {
        val compactReason = compactWhitespace(reason)
        START_SESSION_PANE_ALIAS_REGEX.find(compactReason)?.let { match ->
            return compactStatusText(
                "start failed: pane ${match.groupValues[1]} was still claimed by generation ${match.groupValues[2]} (${match.groupValues[3]})",
            )
        }
        PROJECT_CONTROLLER_COMMAND_FAILED_REGEX.find(compactReason)?.let { match ->
            val command = match.groupValues[1]
            val detail = simplifyRouteFailureDetail(match.groupValues[2])
            val prefix = if (command == "start_session") "start failed" else "$command failed"
            return compactStatusText("$prefix: $detail")
        }
        return compactStatusText("route failed: ${simplifyRouteFailureDetail(compactReason)}")
    }

    internal fun routeFailureDiagnosticsFileForFile(filePath: String): File? {
        val file = File(filePath)
        val root = findAgentDocProjectRoot(file) ?: return null
        val relativePath = relativePathUnder(root, file)
        return TerminalUtil.routeFailureDiagnosticsFile(root.path, relativePath)
    }

    private fun steeringLabel(state: String): String? = when (state) {
        "prompt_target" -> "new prompt"
        "content_edit" -> "document edited"
        "prompt_deleted" -> "prompt deleted"
        "prompt_reduced" -> "prompt reduced"
        else -> null
    }

    private fun routeFailureReason(routeOutput: String): String? {
        val lines = routeOutput
            .lineSequence()
            .map { it.trim() }
            .filter { it.isNotEmpty() }
            .toList()
        val line = lines.firstOrNull { it.startsWith("Error:", ignoreCase = true) }
            ?: lines.firstOrNull {
                it.contains("failed", ignoreCase = true) ||
                    it.contains("refus", ignoreCase = true) ||
                    it.contains("panic", ignoreCase = true)
            }
            ?: lines.firstOrNull()
        return line
            ?.removePrefix("Error:")
            ?.trim()
            ?.ifEmpty { null }
    }

    private fun simplifyRouteFailureDetail(detail: String): String =
        compactWhitespace(detail)
            .removePrefix("refusing ")
            .removePrefix("controller failed to ")
            .ifBlank { "unknown reason" }

    private fun compactStatusText(text: String, maxLength: Int = 120): String {
        val compact = compactWhitespace(text)
        if (compact.length <= maxLength) return compact
        return compact.take(maxLength - 3).trimEnd() + "..."
    }

    private fun compactWhitespace(text: String): String =
        text.replace(Regex("""\s+"""), " ").trim()

    private fun findAgentDocProjectRoot(file: File): File? {
        var current = if (file.isDirectory) file else file.parentFile
        while (current != null) {
            if (File(current, ".agent-doc").isDirectory) {
                return current
            }
            current = current.parentFile
        }
        return null
    }

    private fun relativePathUnder(root: File, file: File): String {
        val relative = try {
            file.canonicalFile.relativeTo(root.canonicalFile).path
        } catch (_: Exception) {
            file.path
        }
        return relative.replace(File.separatorChar, '/')
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
