package com.github.btakita.agentdoc

import com.google.gson.Gson
import com.google.gson.JsonElement
import com.google.gson.JsonObject
import com.google.gson.JsonParser
import com.google.gson.annotations.SerializedName
import com.intellij.openapi.diagnostic.Logger
import javax.swing.SwingUtilities

/**
 * `#jbshrinkrestarts`: what a Run Agent Doc route's output means for the operator, derived by the
 * hot-reloaded native library.
 *
 * `TerminalUtil.sendToTerminal` gets the route's output text and exit code back from the
 * controller. Reading that text (which outcome it is, the paused-queue and deferred-switch
 * details, the notification text, the route snapshot path, and the two success notices) is the
 * native `route_outcome` derivation. The route's wording is the binary's, so a Kotlin copy of
 * these rules needed a plugin generation, an IDE restart on exact-263, for every wording change.
 * The plugin sends the output and the relative path ([evidence]) and executes the answer: the
 * notification type and actions, the ops-log line, and the state projection stay in Kotlin.
 *
 * The route thread asks off the EDT. [frozen] answers only when no native generation does, and
 * fills an output field a library leaves out. It composes the pre-move `TerminalUtil` rules
 * ([TerminalUtil.classifyRunAgentDocRouteFailure], the `parseRunAgentDoc*` parsers, the
 * `buildRunAgentDoc*Message` builders, and the two notice readers) and is never extended.
 */
internal object RouteOutcome {
    private val LOG = Logger.getInstance(RouteOutcome::class.java)
    private val GSON = Gson()

    /** Wire shape of Rust `RouteOutcomeEvidence`. */
    internal data class Evidence(
        @SerializedName("output") val output: String,
        @SerializedName("relative_path") val relativePath: String,
    )

    /** The answer. [summary] is null exactly for a persistent failure. */
    internal data class Result(
        val kind: TerminalUtil.RunAgentDocRouteFailureKind,
        val summary: String?,
        val queuePaused: TerminalUtil.RunAgentDocQueuePaused?,
        val agentSwitchDeferred: TerminalUtil.RunAgentDocAgentSwitchDeferred?,
        val snapshotPath: String,
        val ownedByOtherLaneNotice: String?,
        val paneNotVisibleNotice: String?,
    )

    internal fun evidence(output: String, relativePath: String): Evidence = Evidence(output, relativePath)

    /**
     * Read [output]. Never on the EDT: the route thread asks. An EDT caller is refused the native
     * call and gets the frozen floor.
     */
    internal fun derive(
        output: String,
        relativePath: String,
        derivation: EditorDerivation = EditorDerivation.Native,
    ): Result {
        val floor = frozen(output, relativePath)
        if (SwingUtilities.isEventDispatchThread()) {
            LOG.warn("[route] route_outcome asked on the EDT; using the compatibility floor")
            return floor
        }
        val answer = derivation.derive("route_outcome", GSON.toJson(evidence(output, relativePath))) ?: return floor
        return parseNative(answer, floor) ?: floor
    }

    /**
     * Parse the native output; `null` takes the frozen floor. A kind this plugin generation cannot
     * execute takes the whole floor. A field the answer leaves out (an older library) or cannot be
     * read takes the floor's value for that field.
     */
    internal fun parseNative(json: String, floor: Result): Result? =
        try {
            val root = JsonParser.parseString(json).asJsonObject
            val kind = root.get("kind").stringOrNull()?.let(::kind)
            if (kind == null) {
                null
            } else {
                Result(
                    kind = kind,
                    summary = if (root.has("summary")) root.get("summary").stringOrNull() else floor.summary,
                    queuePaused =
                        if (root.has("queue_paused")) queuePaused(root.get("queue_paused")) else floor.queuePaused,
                    agentSwitchDeferred =
                        if (root.has("agent_switch_deferred")) {
                            agentSwitchDeferred(root.get("agent_switch_deferred"))
                        } else {
                            floor.agentSwitchDeferred
                        },
                    snapshotPath = root.get("snapshot_path").stringOrNull() ?: floor.snapshotPath,
                    ownedByOtherLaneNotice =
                        if (root.has("owned_by_other_lane_notice")) {
                            root.get("owned_by_other_lane_notice").stringOrNull()
                        } else {
                            floor.ownedByOtherLaneNotice
                        },
                    paneNotVisibleNotice =
                        if (root.has("pane_not_visible_notice")) {
                            root.get("pane_not_visible_notice").stringOrNull()
                        } else {
                            floor.paneNotVisibleNotice
                        },
                )
            }
        } catch (e: Exception) {
            LOG.warn("[route] native route_outcome unreadable: ${e.message}")
            null
        }

    private fun JsonElement?.stringOrNull(): String? =
        this?.takeIf { it.isJsonPrimitive && it.asJsonPrimitive.isString }?.asString

    private fun JsonObject.text(name: String): String = get(name).stringOrNull() ?: ""

    private fun JsonObject.flag(name: String): Boolean =
        get(name)?.takeIf { it.isJsonPrimitive && it.asJsonPrimitive.isBoolean }?.asBoolean ?: false

    private fun queuePaused(element: JsonElement?): TerminalUtil.RunAgentDocQueuePaused? {
        val value = element?.takeIf { it.isJsonObject }?.asJsonObject ?: return null
        return TerminalUtil.RunAgentDocQueuePaused(
            reason = value.text("reason"),
            restartSupervisorRedirect = value.flag("restart_supervisor_redirect"),
            stalePid = value.text("stale_pid"),
        )
    }

    private fun agentSwitchDeferred(element: JsonElement?): TerminalUtil.RunAgentDocAgentSwitchDeferred? {
        val value = element?.takeIf { it.isJsonObject }?.asJsonObject ?: return null
        return TerminalUtil.RunAgentDocAgentSwitchDeferred(
            previousHarness = value.text("previous_harness"),
            targetHarness = value.text("target_harness"),
            queuePaused = value.flag("queue_paused"),
            forceRequired = value.flag("force_required"),
            supervisorUnavailable = value.flag("supervisor_unavailable"),
            restartInFlight = value.flag("restart_in_flight"),
        )
    }

    /** The wire token of each kind this plugin generation can execute. */
    internal fun token(kind: TerminalUtil.RunAgentDocRouteFailureKind): String = kind.name.lowercase()

    private fun kind(token: String): TerminalUtil.RunAgentDocRouteFailureKind? =
        TerminalUtil.RunAgentDocRouteFailureKind.values().firstOrNull { token(it) == token }

    /**
     * Compatibility floor: the pre-move `TerminalUtil` route-output rules, frozen. Used only when
     * no native generation answers. Never extended; change the native derivation instead.
     */
    internal fun frozen(output: String, relativePath: String): Result {
        val kind = TerminalUtil.classifyRunAgentDocRouteFailure(output)
        val queuePaused = TerminalUtil.parseRunAgentDocQueuePaused(output)
        val agentSwitchDeferred = TerminalUtil.parseRunAgentDocAgentSwitchDeferred(output)
        val summary =
            when (kind) {
                TerminalUtil.RunAgentDocRouteFailureKind.PERSISTENT -> null
                TerminalUtil.RunAgentDocRouteFailureKind.BUSY_RUNNING,
                TerminalUtil.RunAgentDocRouteFailureKind.STARTUP_NOT_READY,
                -> TerminalUtil.buildRunAgentDocStillRunningMessage(relativePath)
                TerminalUtil.RunAgentDocRouteFailureKind.QUEUED_PENDING ->
                    TerminalUtil.buildRunAgentDocQueuedMessage(relativePath)
                TerminalUtil.RunAgentDocRouteFailureKind.STEERING_DELIVERED ->
                    TerminalUtil.buildRunAgentDocSteeringDeliveredMessage(relativePath)
                TerminalUtil.RunAgentDocRouteFailureKind.QUEUE_PAUSED ->
                    TerminalUtil.buildRunAgentDocQueuePausedMessage(
                        relativePath,
                        queuePaused ?: TerminalUtil.RunAgentDocQueuePaused("", false, ""),
                    )
                TerminalUtil.RunAgentDocRouteFailureKind.AGENT_SWITCH_DEFERRED ->
                    TerminalUtil.buildRunAgentDocAgentSwitchDeferredMessage(
                        relativePath,
                        agentSwitchDeferred ?: TerminalUtil.RunAgentDocAgentSwitchDeferred("", "", false, false, false),
                    )
                TerminalUtil.RunAgentDocRouteFailureKind.DISPATCH_START_UNPROVEN ->
                    TerminalUtil.buildRunAgentDocDispatchUnprovenMessage(relativePath, output)
                TerminalUtil.RunAgentDocRouteFailureKind.PROTECTED_PROMPT_INPUT ->
                    TerminalUtil.buildRunAgentDocProtectedPromptInputMessage(relativePath, output)
            }
        return Result(
            kind = kind,
            summary = summary,
            queuePaused = queuePaused,
            agentSwitchDeferred = agentSwitchDeferred,
            snapshotPath = TerminalUtil.routeSnapshotPath(output),
            ownedByOtherLaneNotice = TerminalUtil.routeOwnedByOtherLaneNotice(output),
            paneNotVisibleNotice = TerminalUtil.routePaneNotVisibleNotice(output),
        )
    }
}
