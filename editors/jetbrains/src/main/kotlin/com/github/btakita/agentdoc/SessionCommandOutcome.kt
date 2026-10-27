package com.github.btakita.agentdoc

import com.google.gson.Gson
import com.google.gson.JsonElement
import com.google.gson.JsonObject
import com.google.gson.JsonParser
import com.google.gson.annotations.SerializedName
import com.intellij.openapi.diagnostic.Logger
import javax.swing.SwingUtilities

/**
 * `#jbsessioncmdoutcome`: what a Clear Session Context or Restart session command's output means
 * for the operator, derived by the hot-reloaded native library.
 *
 * `TerminalUtil.clearSessionContext` and `TerminalUtil.runRestartCommand` get the command's
 * output text and exit status back. Reading that text (whether a clear was queued, deferred, or
 * refused; whether a restart was refused by an editor holding the pane, a busy pane, or a
 * starting actor; the details of each; and the notification text) is the native
 * `session_command_outcome` derivation. The wording is the binary's, so a Kotlin copy of these
 * rules needed a plugin generation, an IDE restart on exact-263, for every wording change. The
 * plugin sends the command, whether it succeeded, whether it was forced, the output, and the
 * relative path ([evidence]) and executes the answer: the notification type and actions, the
 * restart telemetry read, and the clear-command bookkeeping stay in Kotlin.
 *
 * The command thread asks off the EDT. [frozen] answers only when no native generation does, and
 * fills a field a library leaves out. It composes the pre-move `TerminalUtil` rules
 * ([TerminalUtil.parseLiveTurnQueuedClear], [TerminalUtil.isDeferredQueuePreemptClear],
 * [TerminalUtil.parseBusySessionClearRefusal], the three restart refusal parsers, and their
 * `build*Message` builders) and is never extended.
 */
internal object SessionCommandOutcome {
    private val LOG = Logger.getInstance(SessionCommandOutcome::class.java)
    private val GSON = Gson()

    /** The session commands whose output this derivation reads. */
    internal enum class Command(val token: String) {
        CLEAR("clear"),
        RESTART("restart"),
    }

    /** The outcome kinds this plugin generation can execute; [token] is the wire name. */
    internal enum class Kind {
        GENERIC,
        CLEARED,
        CLEAR_QUEUED,
        CLEAR_DEFERRED,
        CLEAR_REFUSED,
        RESTART_REFUSED_EDITOR_HOLDS_PANE,
        RESTART_REFUSED_BUSY,
        RESTART_REFUSED_STARTING,
    }

    /** Wire shape of Rust `SessionCommandOutcomeEvidence`. */
    internal data class Evidence(
        @SerializedName("command") val command: String,
        @SerializedName("succeeded") val succeeded: Boolean,
        @SerializedName("force") val force: Boolean,
        @SerializedName("output") val output: String,
        @SerializedName("relative_path") val relativePath: String,
    )

    /**
     * The answer. [summary] is null exactly for [Kind.GENERIC]; the detail its kind names is set
     * and every other detail is null.
     */
    internal data class Result(
        val kind: Kind,
        val summary: String?,
        val clearQueued: TerminalUtil.LiveTurnQueuedClear? = null,
        val clearRefused: TerminalUtil.BusySessionClearRefusal? = null,
        val restartRefusedBusy: TerminalUtil.BusySessionRestartRefusal? = null,
        val restartRefusedStarting: TerminalUtil.StartingSessionRestartRefusal? = null,
        val restartRefusedEditorHoldsPane: TerminalUtil.EditorHoldsPaneRestartRefusal? = null,
    )

    internal fun evidence(
        command: Command,
        succeeded: Boolean,
        force: Boolean,
        output: String,
        relativePath: String,
    ): Evidence = Evidence(command.token, succeeded, force, output, relativePath)

    /**
     * Read [output]. Never on the EDT: the command thread asks. An EDT caller is refused the
     * native call and gets the frozen floor.
     */
    internal fun derive(
        command: Command,
        succeeded: Boolean,
        force: Boolean,
        output: String,
        relativePath: String,
        derivation: EditorDerivation = EditorDerivation.Native,
    ): Result {
        val floor = frozen(command, succeeded, force, output, relativePath)
        if (SwingUtilities.isEventDispatchThread()) {
            LOG.warn("[session] session_command_outcome asked on the EDT; using the compatibility floor")
            return floor
        }
        val input = GSON.toJson(evidence(command, succeeded, force, output, relativePath))
        val answer = derivation.derive("session_command_outcome", input) ?: return floor
        return parseNative(answer, floor) ?: floor
    }

    /**
     * Parse the native output; `null` takes the frozen floor. A kind this plugin generation cannot
     * execute, or a refusal kind whose details cannot be read, takes the whole floor. A summary the
     * answer leaves out (an older library) takes the floor's.
     */
    internal fun parseNative(json: String, floor: Result): Result? =
        try {
            val root = JsonParser.parseString(json).asJsonObject
            val kind = root.get("kind").stringOrNull()?.let(::kind)
            if (kind == null) {
                null
            } else {
                val result = Result(
                    kind = kind,
                    summary = if (root.has("summary")) root.get("summary").stringOrNull() else floor.summary,
                    clearQueued = clearQueued(root.get("clear_queued")),
                    clearRefused = clearRefused(root.get("clear_refused")),
                    restartRefusedBusy = restartRefusedBusy(root.get("restart_refused_busy")),
                    restartRefusedStarting = restartRefusedStarting(root.get("restart_refused_starting")),
                    restartRefusedEditorHoldsPane =
                        restartRefusedEditorHoldsPane(root.get("restart_refused_editor_holds_pane")),
                )
                result.takeIf { executable(it) }
            }
        } catch (e: Exception) {
            LOG.warn("[session] native session_command_outcome unreadable: ${e.message}")
            null
        }

    /** A refusal kind is executable only with its details; the other kinds need none. */
    private fun executable(result: Result): Boolean =
        when (result.kind) {
            Kind.CLEAR_QUEUED -> result.clearQueued != null
            Kind.CLEAR_REFUSED -> result.clearRefused != null
            Kind.RESTART_REFUSED_EDITOR_HOLDS_PANE -> result.restartRefusedEditorHoldsPane != null
            Kind.RESTART_REFUSED_BUSY -> result.restartRefusedBusy != null
            Kind.RESTART_REFUSED_STARTING -> result.restartRefusedStarting != null
            Kind.GENERIC, Kind.CLEARED, Kind.CLEAR_DEFERRED -> true
        }

    private fun JsonElement?.stringOrNull(): String? =
        this?.takeIf { it.isJsonPrimitive && it.asJsonPrimitive.isString }?.asString

    private fun JsonElement?.objectOrNull(): JsonObject? = this?.takeIf { it.isJsonObject }?.asJsonObject

    private fun JsonObject.text(name: String): String = get(name).stringOrNull() ?: ""

    private fun JsonObject.flag(name: String): Boolean =
        get(name)?.takeIf { it.isJsonPrimitive && it.asJsonPrimitive.isBoolean }?.asBoolean ?: false

    private fun JsonObject.long(name: String): Long? =
        get(name)?.takeIf { it.isJsonPrimitive && it.asJsonPrimitive.isNumber }?.asLong

    private fun clearQueued(element: JsonElement?): TerminalUtil.LiveTurnQueuedClear? {
        val value = element.objectOrNull() ?: return null
        return TerminalUtil.LiveTurnQueuedClear(
            file = value.text("file"),
            pane = value.text("pane"),
            evidence = value.text("evidence"),
            evidenceAgeSecs = value.long("evidence_age_secs"),
            rearmed = value.flag("rearmed"),
            settlesInSecs = value.long("settles_in_secs"),
            preTurnGraceSecs = value.long("pre_turn_grace_secs"),
        )
    }

    private fun clearRefused(element: JsonElement?): TerminalUtil.BusySessionClearRefusal? {
        val value = element.objectOrNull() ?: return null
        return TerminalUtil.BusySessionClearRefusal(
            file = value.text("file"),
            pane = value.text("pane"),
            source = value.text("source"),
            currentCommand = value.text("current_command"),
            tail = value.text("tail"),
            protectedReason = value.text("protected_reason"),
            liveTurnEvidence = value.text("live_turn_evidence"),
        )
    }

    private fun restartRefusedBusy(element: JsonElement?): TerminalUtil.BusySessionRestartRefusal? {
        val value = element.objectOrNull() ?: return null
        return TerminalUtil.BusySessionRestartRefusal(
            file = value.text("file"),
            pane = value.text("pane"),
            source = value.text("source"),
            currentCommand = value.text("current_command"),
            tail = value.text("tail"),
        )
    }

    private fun restartRefusedStarting(element: JsonElement?): TerminalUtil.StartingSessionRestartRefusal? {
        val value = element.objectOrNull() ?: return null
        return TerminalUtil.StartingSessionRestartRefusal(file = value.text("file"), reason = value.text("reason"))
    }

    private fun restartRefusedEditorHoldsPane(element: JsonElement?): TerminalUtil.EditorHoldsPaneRestartRefusal? {
        val value = element.objectOrNull() ?: return null
        return TerminalUtil.EditorHoldsPaneRestartRefusal(
            file = value.text("file"),
            pane = value.text("pane"),
            editor = value.text("editor"),
            source = value.text("source"),
            currentCommand = value.text("current_command"),
            tail = value.text("tail"),
        )
    }

    /** The wire token of each kind this plugin generation can execute. */
    internal fun token(kind: Kind): String = kind.name.lowercase()

    private fun kind(token: String): Kind? = Kind.values().firstOrNull { token(it) == token }

    /**
     * Compatibility floor: the pre-move `TerminalUtil` session-command output rules, frozen. Used
     * only when no native generation answers. Never extended; change the native derivation instead.
     */
    internal fun frozen(
        command: Command,
        succeeded: Boolean,
        force: Boolean,
        output: String,
        relativePath: String,
    ): Result =
        when (command) {
            Command.CLEAR -> frozenClear(succeeded, output, relativePath)
            Command.RESTART -> frozenRestart(succeeded, force, output, relativePath)
        }

    private fun frozenClear(succeeded: Boolean, output: String, relativePath: String): Result {
        if (succeeded) {
            TerminalUtil.parseLiveTurnQueuedClear(output)?.let { queued ->
                return Result(
                    Kind.CLEAR_QUEUED,
                    TerminalUtil.buildLiveTurnQueuedClearMessage(relativePath, queued),
                    clearQueued = queued,
                )
            }
            if (TerminalUtil.isDeferredQueuePreemptClear(output)) {
                return Result(Kind.CLEAR_DEFERRED, TerminalUtil.buildDeferredQueuePreemptClearMessage(relativePath))
            }
            return Result(Kind.CLEARED, output.ifBlank { "Cleared session context for $relativePath" })
        }
        val refusal = TerminalUtil.parseBusySessionClearRefusal(output) ?: return Result(Kind.GENERIC, null)
        return Result(
            Kind.CLEAR_REFUSED,
            TerminalUtil.buildBusySessionClearBlockedMessage(relativePath, refusal),
            clearRefused = refusal,
        )
    }

    private fun frozenRestart(succeeded: Boolean, force: Boolean, output: String, relativePath: String): Result {
        if (succeeded) return Result(Kind.GENERIC, null)
        TerminalUtil.parseEditorHoldsPaneRestartRefusal(output)?.let { refusal ->
            return Result(
                Kind.RESTART_REFUSED_EDITOR_HOLDS_PANE,
                TerminalUtil.buildEditorHoldsPaneRestartBlockedMessage(relativePath, refusal),
                restartRefusedEditorHoldsPane = refusal,
            )
        }
        if (force) return Result(Kind.GENERIC, null)
        TerminalUtil.parseBusySessionRestartRefusal(output)?.let { refusal ->
            return Result(
                Kind.RESTART_REFUSED_BUSY,
                TerminalUtil.buildBusySessionRestartBlockedMessage(relativePath, refusal),
                restartRefusedBusy = refusal,
            )
        }
        TerminalUtil.parseStartingSessionRestartRefusal(output)?.let { refusal ->
            return Result(
                Kind.RESTART_REFUSED_STARTING,
                TerminalUtil.buildStartingSessionRestartBlockedMessage(relativePath, refusal),
                restartRefusedStarting = refusal,
            )
        }
        return Result(Kind.GENERIC, null)
    }
}
