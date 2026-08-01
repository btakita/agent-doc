package com.github.btakita.agentdoc

import com.google.gson.JsonArray
import com.google.gson.JsonObject
import com.google.gson.JsonParser
import com.intellij.openapi.diagnostic.Logger
import io.github.lazily.IpcMessage
import io.github.lazily.NodeKey
import io.github.lazily.NodeSnapshot
import io.github.lazily.NodeState
import io.github.lazily.Snapshot
import java.io.File
import java.net.UnixDomainSocketAddress
import java.nio.channels.Channels
import java.nio.channels.SocketChannel
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

internal fun jsonBooleanFieldOrNull(
    value: JsonObject,
    field: String,
): Boolean? =
    value.get(field)
        ?.takeIf { it.isJsonPrimitive && it.asJsonPrimitive.isBoolean }
        ?.asBoolean

internal fun jsonLongFieldOrNull(
    value: JsonObject,
    field: String,
): Long? =
    value.get(field)
        ?.takeIf { it.isJsonPrimitive && it.asJsonPrimitive.isNumber }
        ?.asLong

internal fun jsonStringFieldOrNull(
    value: JsonObject,
    field: String,
): String? =
    value.get(field)
        ?.takeIf { it.isJsonPrimitive && it.asJsonPrimitive.isString }
        ?.asString

internal fun controllerFailureMessageUtil(root: JsonObject): String? =
    if (jsonBooleanFieldOrNull(root, "ok") == true) {
        null
    } else {
        jsonStringFieldOrNull(root, "error") ?: "Project Controller request failed"
    }

internal data class CpEditorRouteResult(
    val exitCode: Int,
    val output: String,
    val steering: CpTurnSteeringAck? = null,
)

internal data class CpTurnSteeringAck(
    val kind: String,
    val steeringId: String,
    val outcome: String,
    val acceptedBytes: Int,
)

internal data class CpTmuxLayoutSyncState(
    val synced: Boolean,
    val reason: String,
)

internal enum class PaneLayoutPhase(val token: String) {
    NeedsEffect("needs_effect"),
    Applying("applying"),
    RetryPending("retry_pending"),
    Converged("converged");

    companion object {
        fun fromToken(token: String?): PaneLayoutPhase? = entries.firstOrNull { it.token == token }
    }
}

internal enum class PaneLayoutReasonCode(val token: String) {
    Unobserved("unobserved"),
    EffectInFlight("effect_in_flight"),
    ObservedConvergence("observed_convergence"),
    PaneCountMismatch("pane_count_mismatch"),
    PaneOrderMismatch("pane_order_mismatch"),
    TmuxUnavailable("tmux_unavailable"),
    EffectFailed("effect_failed"),
    ObservationFailed("observation_failed"),
    RetryScheduled("retry_scheduled");

    companion object {
        fun fromToken(token: String?): PaneLayoutReasonCode? =
            entries.firstOrNull { it.token == token }
    }
}

internal data class ProjectControllerStateSubscribeResult(
    val documentHash: String,
    val messageJson: String,
    val documentVersion: Long,
    val peerAckRecorded: Boolean,
)

internal data class CpDocumentPathTransitionReceipt(
    val transitionId: String,
    val phase: String,
    val converged: Boolean,
    val attempt: Long,
    val error: String? = null,
)

internal enum class MissingFocusPanePolicy(val token: String) {
    ObserveOnly("observe_only"),
    ResumeLatest("resume_latest"),
}

internal enum class ProjectControllerCommand(val token: String) {
    EditorCommandSubmitAsync("editor_command_submit_async"),
    EditorCommandStatus("editor_command_status"),
    EditorCommandAwait("editor_command_await"),
}

internal enum class EditorCommandName(val token: String) {
    EditorRoute("editor_route"),
    SyncTmuxLayout("sync_tmux_layout"),
    FocusDocumentPane("focus_document_pane"),
}

internal enum class CommandProjectionStatus(val token: String) {
    Submitted("submitted"),
    Accepted("accepted"),
    Running("running"),
    Applied("applied"),
    Rejected("rejected");

    companion object {
        fun fromToken(token: String?): CommandProjectionStatus? =
            entries.firstOrNull { it.token == token }
    }
}

/**
 * High-level editor route RPC over the Project Controller socket.
 */
internal object CpRouteClient {
    private val log = Logger.getInstance(CpRouteClient::class.java)
    private const val PANE_LAYOUT_DESIRED_STATE_CHANNEL = "agent-doc/pane-layout/desired/v1"
    private const val PANE_LAYOUT_DESIRED_TYPE_TAG = "agent-doc.pane-layout.desired.v1"
    private val statePlaneProducerId = "jetbrains-" + java.util.UUID.randomUUID().toString()
    private val statePlaneEpoch = java.util.concurrent.atomic.AtomicLong(0)
    private val editorSurfaceClientId = "jetbrains-pid:${ProcessHandle.current().pid()}"
    private val editorSurfaceGeneration = System.nanoTime().coerceAtLeast(1L)
    private val editorSurfaceSequence = java.util.concurrent.atomic.AtomicLong(0)

    /**
     * Publish one ordered editor fact directly to the already-running Project Controller.
     *
     * This passive lane deliberately bypasses the reloadable native library: the controller owns
     * both the reactive graph and its tmux effect, while the plugin owns only observation capture
     * and socket transport.
     */
    fun observeEditorSurface(
        projectRoot: String,
        surfaceJson: String,
    ): CpEditorRouteResult {
        val socket = cpcSocket(projectRoot)
        val request =
            editorSurfaceObserveRequest(
                surfaceJson = surfaceJson,
                clientId = editorSurfaceClientId,
                generation = editorSurfaceGeneration,
                sequence = editorSurfaceSequence.incrementAndGet(),
            )
        return try {
            val receipt = sendRequestDataToSocket(socket, request)
            CpEditorRouteResult(
                exitCode = 0,
                output = receipt.toString(),
            )
        } catch (e: Exception) {
            log.debug(
                "[layout-sync] editor_surface_observe unavailable via ${socket.path}: ${e.message}",
            )
            CpEditorRouteResult(
                exitCode = 1,
                output = "editor_surface_observe unavailable via ${socket.path}: ${e.message}",
            )
        }
    }

    fun forgetEditorSurface(projectRoot: String): Boolean {
        val socket = cpcSocket(projectRoot)
        val request =
            JsonObject().also {
                it.addProperty("command", "editor_surface_forget")
                it.addProperty("generation", editorSurfaceGeneration)
                it.addProperty("caller", editorSurfaceClientId)
        }
        return try {
            sendRequestDataToSocket(socket, request).get("forgotten")?.asBoolean ?: false
        } catch (e: Exception) {
            log.debug(
                "[layout-sync] editor_surface_forget unavailable via ${socket.path}: ${e.message}",
            )
            false
        }
    }

    fun observeDocumentPathTransition(
        projectRoot: String,
        transitionId: String,
        oldPath: String,
        newPath: String,
    ): CpDocumentPathTransitionReceipt {
        val socket = cpcSocket(projectRoot)
        val request =
            documentPathTransitionRequest(
                transitionId = transitionId,
                oldPath = oldPath,
                newPath = newPath,
                clientId = editorSurfaceClientId,
                generation = editorSurfaceGeneration,
                sequence = editorSurfaceSequence.incrementAndGet(),
            )
        return try {
            val receipt = sendRequestDataToSocket(socket, request)
            CpDocumentPathTransitionReceipt(
                transitionId =
                    jsonStringFieldOrNull(receipt, "transition_id") ?: transitionId,
                phase = jsonStringFieldOrNull(receipt, "phase") ?: "retry_pending",
                converged = jsonBooleanFieldOrNull(receipt, "converged") == true,
                attempt = jsonLongFieldOrNull(receipt, "attempt") ?: 0L,
                error = jsonStringFieldOrNull(receipt, "error"),
            )
        } catch (e: Exception) {
            CpDocumentPathTransitionReceipt(
                transitionId = transitionId,
                phase = "retry_pending",
                converged = false,
                attempt = 0,
                error =
                    "document_path_transition_observe unavailable via ${socket.path}: ${e.message}",
            )
        }
    }

    /**
     * Read the controller's current in-memory turn projection for one document.
     *
     * The returned shape matches [TurnStateBridge.presentationFromDocumentAuthority] so the UI
     * keeps one fail-closed rendering path without entering the native library or opening SQLite.
     */
    fun documentTurnAuthority(
        projectRoot: String,
        filePath: String,
    ): String {
        val socket = cpcSocket(projectRoot)
        return try {
            val turn = sendRequestDataToSocket(socket, documentTurnProjectionRequest(filePath))
            JsonObject().also {
                it.addProperty("document", filePath)
                it.addProperty("readiness", "ready")
                it.add("turn", turn)
            }.toString()
        } catch (e: Exception) {
            JsonObject().also {
                it.addProperty("document", filePath)
                it.addProperty("readiness", "unavailable")
                it.addProperty(
                    "error",
                    "document_turn_projection unavailable via ${socket.path}: ${e.message}",
                )
            }.toString()
        }
    }

    internal fun documentTurnProjectionRequest(filePath: String): JsonObject =
        JsonObject().also {
            it.addProperty("command", "document_turn_projection")
            it.addProperty("file", filePath)
            it.addProperty("caller", EditorIdentity.id)
        }

    internal fun editorSurfaceObserveRequest(
        surfaceJson: String,
        clientId: String,
        generation: Long,
        sequence: Long,
    ): JsonObject {
        val surface = JsonParser.parseString(surfaceJson).asJsonObject
        val observation =
            JsonObject().also {
                it.addProperty("client_id", clientId)
                it.addProperty("generation", generation)
                it.addProperty("sequence", sequence)
                it.add("surface", surface)
            }
        return JsonObject().also {
            it.addProperty("command", "editor_surface_observe")
            it.addProperty("file", surface.get("focused").asString)
            it.addProperty("generation", generation)
            it.addProperty("caller", clientId)
            it.addProperty("reason", "editor_surface_observation")
            it.addProperty("diagnostic_payload", observation.toString())
        }
    }

    internal fun documentPathTransitionRequest(
        transitionId: String,
        oldPath: String,
        newPath: String,
        clientId: String,
        generation: Long,
        sequence: Long,
    ): JsonObject {
        val observation =
            JsonObject().also {
                it.addProperty("transition_id", transitionId)
                it.addProperty("client_id", clientId)
                it.addProperty("generation", generation)
                it.addProperty("sequence", sequence)
                it.addProperty("old_path", oldPath)
                it.addProperty("new_path", newPath)
            }
        return JsonObject().also {
            it.addProperty("command", "document_path_transition_observe")
            it.addProperty("file", newPath)
            it.addProperty("generation", generation)
            it.addProperty("caller", clientId)
            it.addProperty("reason", "editor_vfs_path_transition")
            it.addProperty("diagnostic_payload", observation.toString())
        }
    }

    fun runEditorRoute(
        projectRoot: String,
        filePath: String,
        relativePath: String,
        layoutArgs: List<String>,
        waitForReadySeconds: Long,
        attemptId: String?,
        routeKey: String?,
        selectedText: String? = null,
        steeringId: String? = null,
    ): CpEditorRouteResult {
        val socket = cpcSocket(projectRoot)
        if (commandPlaneEnabled()) {
            val commandId = "cmd-" + java.util.UUID.randomUUID().toString()
            val request = editorCommandSubmitRequest(
                filePath = filePath,
                relativePath = relativePath,
                layoutArgs = layoutArgs,
                waitForReadySeconds = waitForReadySeconds,
            attemptId = attemptId,
            routeKey = routeKey,
                commandId = commandId,
                controllerCommand = ProjectControllerCommand.EditorCommandSubmitAsync.token,
                selectedText = selectedText,
                steeringId = steeringId,
            )
        return try {
            val accepted = sendAcceptedCommandSubmitToSocket(
                socket,
                request,
                commandId,
                EditorCommandName.EditorRoute.token,
            )
            if (accepted.exitCode != 0) {
                accepted
            } else {
                awaitCommandSubmitTerminal(
                    socket = socket,
                    filePath = filePath,
                    commandId = commandId,
                    timeoutMs = waitForReadySeconds * 1000 + COMMAND_COMPLETION_GRACE_MS,
                    commandName = EditorCommandName.EditorRoute.token,
                )
            }
        } catch (e: Exception) {
            log.warn("[route] command-plane editor_route request failed via ${socket.path}: ${e.message}")
                CpEditorRouteResult(
                    exitCode = 1,
                    output = "command-plane editor_route request failed via ${socket.path}: ${e.message}",
                )
            }
        }
        val request = editorRouteRequest(
            filePath = filePath,
            relativePath = relativePath,
            layoutArgs = layoutArgs,
            waitForReadySeconds = waitForReadySeconds,
            attemptId = attemptId,
            routeKey = routeKey,
            selectedText = selectedText,
            steeringId = steeringId,
        )
        return try {
            sendToSocket(socket, request)
        } catch (e: Exception) {
            log.warn("[route] Project Controller editor_route request failed via ${socket.path}: ${e.message}")
            CpEditorRouteResult(
                exitCode = 1,
                output = "Project Controller editor_route request failed via ${socket.path}: ${e.message}",
            )
        }
    }

    fun submitSyncTmuxLayout(
        projectRoot: String,
        columnsJson: String,
        window: String? = null,
        focus: String?,
        noAutostart: Boolean,
        exactVisible: Boolean,
        callerKind: String,
    ): CpEditorRouteResult {
        val socket = cpcSocket(projectRoot)
        val request = paneLayoutDesiredStatePublishRequest(
            projectRoot = projectRoot,
            columnsJson = columnsJson,
            window = window,
            focus = focus,
            noAutostart = noAutostart,
            exactVisible = exactVisible,
            callerKind = callerKind,
        )
        return try {
            val data = sendRequestDataToSocket(socket, request)
            val publishedPlaneVersion = jsonLongFieldOrNull(data, "plane_version")
            val accepted = if (
                jsonBooleanFieldOrNull(data, "accepted") == true &&
                publishedPlaneVersion != null &&
                publishedPlaneVersion > 0
            ) {
                CpEditorRouteResult(
                    exitCode = 0,
                    output = "pane layout projection published " +
                        "plane_version=$publishedPlaneVersion",
                )
            } else {
                CpEditorRouteResult(
                    exitCode = 1,
                    output = jsonStringFieldOrNull(data, "reason")
                        ?: "pane layout projection was not accepted with a valid plane version",
                )
            }
            // Publishing the latest desired snapshot is the editor-side completion
            // boundary. The controller owns retained, reactive reconciliation from
            // here; waiting for one exact version couples the action to an obsolete
            // snapshot whenever a newer editor observation supersedes it.
            accepted
        } catch (e: Exception) {
            log.warn("[sync] pane-layout state projection publish failed via ${socket.path}: ${e.message}")
            CpEditorRouteResult(
                exitCode = 1,
                output = "pane-layout state projection publish failed via ${socket.path}: ${e.message}",
            )
        }
    }

    fun submitFocusDocumentPane(
        projectRoot: String,
        documentPath: String,
    ): CpEditorRouteResult {
        val socket = cpcSocket(projectRoot)
        val commandId = "cmd-" + java.util.UUID.randomUUID().toString()
        val request = focusDocumentPaneCommandSubmitRequest(
            projectRoot = projectRoot,
            documentPath = documentPath,
            commandId = commandId,
            controllerCommand = ProjectControllerCommand.EditorCommandSubmitAsync.token,
        )
        return try {
                sendAcceptedCommandSubmitToSocket(
                    socket,
                    request,
                    commandId,
                    EditorCommandName.FocusDocumentPane.token,
                )
        } catch (e: Exception) {
            log.warn("[focus] command-plane focus_document_pane submit failed via ${socket.path}: ${e.message}")
            CpEditorRouteResult(
                exitCode = 1,
                output = "command-plane focus_document_pane submit failed via ${socket.path}: ${e.message}",
            )
        }
    }

    fun tmuxLayoutSyncState(
        projectRoot: String,
        columnsJson: String,
        focus: String?,
    ): CpTmuxLayoutSyncState? {
        val socket = cpcSocket(projectRoot)
        val request = tmuxLayoutSyncStateRequest(columnsJson, focus)
        return try {
            val data = sendRequestDataToSocket(socket, request)
            CpTmuxLayoutSyncState(
                synced = data.get("synced")?.asBoolean ?: false,
                reason = data.get("reason")?.asString ?: "missing_reason",
            )
        } catch (e: Exception) {
            log.debug("[sync] tmux_layout_sync_state request failed via ${socket.path}: ${e.message}")
            null
        }
    }

    internal fun tmuxLayoutSyncStateRequest(
        columnsJson: String,
        focus: String?,
    ): JsonObject {
        val payload = JsonObject()
        payload.add("columns", JsonParser.parseString(columnsJson).asJsonArray)
        focus?.let { payload.addProperty("focus", it) }
        return JsonObject().also {
            it.addProperty("command", "tmux_layout_sync_state")
            it.addProperty("diagnostic_payload", payload.toString())
        }
    }

    fun tmuxFocusState(projectRoot: String): String? {
        val socket = cpcSocket(projectRoot)
        val request = JsonObject().also {
            it.addProperty("command", "tmux_focus_state")
        }
        return try {
            sendRequestDataToSocket(socket, request).toString()
        } catch (e: Exception) {
            log.debug("[focus] tmux_focus_state request failed via ${socket.path}: ${e.message}")
            null
        }
    }

    fun stateSubscribe(
        projectRoot: String,
        filePath: String,
        documentHash: String,
        lastEpoch: Long,
        ackedVersion: Long,
    ): ProjectControllerStateSubscribeResult {
        val socket = cpcSocket(projectRoot)
        val request = JsonObject().also {
            it.addProperty("command", "state_subscribe")
            it.addProperty("file", filePath)
            it.addProperty("generation", lastEpoch)
            it.addProperty(
                "diagnostic_payload",
                JsonObject().also { payload ->
                    payload.addProperty("document_hash", documentHash)
                    payload.addProperty("peer_pid", ProcessHandle.current().pid())
                    payload.addProperty("editor_id", EditorIdentity.id)
                    payload.addProperty("acked_version", ackedVersion)
                }.toString(),
            )
        }
        val data = sendRequestDataToSocket(socket, request)
        val returnedHash = data.get("document_hash")?.asString ?: documentHash
        val message = data.get("message")
            ?: throw IllegalStateException("Project Controller state_subscribe response missing message")
        val documentVersion = data.get("document_version")?.asLong
            ?: throw IllegalStateException("Project Controller state_subscribe response missing document_version")
        return ProjectControllerStateSubscribeResult(
            returnedHash,
            message.toString(),
            documentVersion,
            data.get("peer_ack_recorded")?.asBoolean ?: false,
        )
    }

    // The `agent-doc.editor_route.v1` payload the controller consumes, shared by
    // the classic `editor_route` request and the command-plane submit.
    internal fun editorRoutePayload(
        relativePath: String,
        layoutArgs: List<String>,
        waitForReadySeconds: Long,
        attemptId: String?,
        routeKey: String?,
        selectedText: String? = null,
        steeringId: String? = null,
    ): JsonObject {
        val payload = JsonObject()
        payload.addProperty("source", "jetbrains_plugin")
        payload.addProperty("relative_path", relativePath)
        payload.addProperty("dispatch_only", true)
        payload.addProperty("plain_trigger", true)
        payload.addProperty("wait_for_ready_secs", waitForReadySeconds)
        payload.add("layout_args", JsonArray().also { array ->
            layoutArgs.forEach { array.add(it) }
        })
        attemptId?.let { payload.addProperty("attempt_id", it) }
        routeKey?.let { payload.addProperty("route_key", it) }
        selectedText?.let { payload.addProperty("selected_text", it) }
        steeringId?.let { payload.addProperty("steering_id", it) }
        return payload
    }

    internal fun editorRouteRequest(
        filePath: String,
        relativePath: String,
        layoutArgs: List<String>,
        waitForReadySeconds: Long,
        attemptId: String?,
        routeKey: String?,
        selectedText: String? = null,
        steeringId: String? = null,
    ): JsonObject {
        val payload = editorRoutePayload(
            relativePath,
            layoutArgs,
            waitForReadySeconds,
            attemptId,
            routeKey,
            selectedText,
            steeringId,
        )
        val request = JsonObject()
        request.addProperty("command", EditorCommandName.EditorRoute.token)
        request.addProperty("file", filePath)
        request.addProperty("diagnostic_payload", payload.toString())
        return request
    }

    // #lzmsgpcp: route through the lazily command/RPC message plane
    // (`command-plane-v1`). Phase 7 gate 3 — default-on; the controller keeps both
    // endpoints in shadow mode, so `AGENT_DOC_COMMAND_PLANE=0` falls back to the
    // classic `editor_route` path.
    internal fun commandPlaneEnabled(): Boolean = System.getenv("AGENT_DOC_COMMAND_PLANE") != "0"

    private fun sha256Hex(bytes: ByteArray): String =
        java.security.MessageDigest.getInstance("SHA-256").digest(bytes)
            .joinToString("") { "%02x".format(it) }

    // Build a `CommandSubmit` envelope (namespace `agent-doc`, name `editor_route`)
    // carrying the inline `editor_route` payload. Mirrors lazily-kt
    // `CommandSubmit.toJson()` / `schemas/message-passing.json`.
    internal fun editorCommandSubmitRequest(
        filePath: String,
        relativePath: String,
        layoutArgs: List<String>,
        waitForReadySeconds: Long,
        attemptId: String?,
    routeKey: String?,
        commandId: String,
        controllerCommand: String = "editor_command_submit",
        selectedText: String? = null,
        steeringId: String? = null,
    ): JsonObject {
        val payload = editorRoutePayload(
            relativePath,
            layoutArgs,
            waitForReadySeconds,
            attemptId,
            routeKey,
            selectedText,
            steeringId,
        )
        return commandSubmitRequest(
            filePath = filePath,
            name = EditorCommandName.EditorRoute.token,
            payloadType = "agent-doc.editor_route.v1",
            payload = payload,
            // Retries of one click share an attempt id; a later intentional click
            // gets a new id. Durable controller receipts still coalesce attempts
            // while the prior document turn is in flight.
            idempotencyKey = steeringId ?: attemptId ?: routeKey ?: relativePath,
            commandId = commandId,
        deadlineMs = waitForReadySeconds * 1000,
        supersede = false,
        controllerCommand = controllerCommand,
    )
}

internal fun editorCommandStatusRequest(filePath: String, commandId: String): JsonObject {
    val request = JsonObject()
    request.addProperty("command", ProjectControllerCommand.EditorCommandStatus.token)
    request.addProperty("file", filePath)
    request.addProperty(
        "diagnostic_payload",
        JsonObject().also { it.addProperty("command_id", commandId) }.toString(),
    )
    return request
}

internal fun editorCommandAwaitRequest(
    filePath: String,
    commandId: String,
    timeoutMs: Long,
): JsonObject {
    val request = JsonObject()
    request.addProperty("command", ProjectControllerCommand.EditorCommandAwait.token)
    request.addProperty("file", filePath)
    request.addProperty(
        "diagnostic_payload",
        JsonObject().also {
            it.addProperty("command_id", commandId)
            it.addProperty("timeout_ms", timeoutMs.coerceAtLeast(MIN_COMMAND_AWAIT_TIMEOUT_MS))
        }.toString(),
    )
    return request
}

    internal fun paneLayoutDesiredStatePublishRequest(
        projectRoot: String,
        columnsJson: String,
        window: String?,
        focus: String?,
        noAutostart: Boolean,
        exactVisible: Boolean,
        callerKind: String = "manual",
        producerId: String = statePlaneProducerId,
        epoch: Long = statePlaneEpoch.incrementAndGet(),
    ): JsonObject {
        val desired = JsonObject()
        desired.addProperty("project_root", projectRoot)
        desired.add("columns", JsonParser.parseString(columnsJson).asJsonArray)
        window?.let { desired.addProperty("window", it) }
        focus?.let { desired.addProperty("focus", it) }
        desired.addProperty("no_autostart", noAutostart)
        desired.addProperty("exact_visible", exactVisible)
        desired.addProperty("caller_kind", callerKind)
        val node = NodeSnapshot.payload(
            node = 1L,
            typeTag = PANE_LAYOUT_DESIRED_TYPE_TAG,
            bytes = desired.toString().encodeToByteArray(),
        ).withKey(NodeKey.from(PANE_LAYOUT_DESIRED_STATE_CHANNEL))
        val messageJson = IpcMessage.ofSnapshot(
            Snapshot(
                epoch = epoch,
                nodes = listOf(node),
                roots = listOf(1L),
            ),
        ).encodeJson().decodeToString()
        val publication = JsonObject()
        publication.addProperty("channel", PANE_LAYOUT_DESIRED_STATE_CHANNEL)
        publication.addProperty("producer_id", producerId)
        publication.addProperty("message_json", messageJson)
        return JsonObject().also {
            it.addProperty("command", "state_plane_publish")
            it.addProperty("diagnostic_payload", publication.toString())
        }
    }

    internal fun statePlaneSubscribeRequest(
        channel: String,
        afterVersion: Long,
        timeoutMs: Long,
    ): JsonObject {
        val subscription = JsonObject()
        subscription.addProperty("channel", channel)
        subscription.addProperty("after_version", afterVersion)
        subscription.addProperty("timeout_ms", timeoutMs)
        return JsonObject().also {
            it.addProperty("command", "state_plane_subscribe")
            it.addProperty("diagnostic_payload", subscription.toString())
        }
    }

    internal fun syncTmuxLayoutCommandSubmitRequest(
        projectRoot: String,
        columnsJson: String,
        window: String?,
        focus: String?,
        noAutostart: Boolean,
        exactVisible: Boolean,
        commandId: String,
        callerKind: String = "manual",
        controllerCommand: String = "editor_command_submit",
    ): JsonObject {
        val payload = JsonObject()
        payload.addProperty("project_root", projectRoot)
        payload.add("columns", JsonParser.parseString(columnsJson).asJsonArray)
        window?.let { payload.addProperty("window", it) }
        focus?.let { payload.addProperty("focus", it) }
        payload.addProperty("no_autostart", noAutostart)
        payload.addProperty("exact_visible", exactVisible)
        payload.addProperty("caller_kind", callerKind)
        return commandSubmitRequest(
            filePath = focus ?: projectRoot,
            name = EditorCommandName.SyncTmuxLayout.token,
            payloadType = "agent-doc.sync_tmux_layout.v1",
            payload = payload,
            idempotencyKey = "$projectRoot:sync",
            commandId = commandId,
            deadlineMs = 35_000,
            supersede = true,
            controllerCommand = controllerCommand,
        )
    }

    internal fun focusDocumentPaneCommandSubmitRequest(
        projectRoot: String,
        documentPath: String,
        commandId: String,
        controllerCommand: String = "editor_command_submit",
    ): JsonObject {
        val payload = JsonObject()
        payload.addProperty("project_root", projectRoot)
payload.addProperty("document_path", documentPath)
payload.addProperty("no_promotion", true)
payload.addProperty("active_window_guard", true)
// The low-latency selection lane only focuses an already-visible pane.
// Surface reconciliation owns pane recovery/creation; doing that work here
// made a stale focus request slow enough to land after the operator moved on.
payload.addProperty("missing_pane_policy", MissingFocusPanePolicy.ObserveOnly.token)
        return commandSubmitRequest(
            filePath = documentPath,
            name = EditorCommandName.FocusDocumentPane.token,
            payloadType = "agent-doc.focus_document_pane.v1",
            payload = payload,
            // Selection focus is one latest-wins intent per project. A
            // document-specific key lets rapid A -> B -> C navigation queue
            // three independent pane selections and an older command can land
            // after the operator has moved on.
            idempotencyKey = "$projectRoot:selected-document-focus",
            commandId = commandId,
deadlineMs = 750,
            supersede = true,
            controllerCommand = controllerCommand,
        )
    }

    private fun commandSubmitRequest(
        filePath: String,
        name: String,
        payloadType: String,
        payload: JsonObject,
        idempotencyKey: String,
        commandId: String,
        deadlineMs: Long,
        supersede: Boolean,
        controllerCommand: String = "editor_command_submit",
    ): JsonObject {
        val payloadBytes = payload.toString().toByteArray(Charsets.UTF_8)
        val submit = JsonObject()
        submit.addProperty("command_id", commandId)
        submit.addProperty("causation_id", commandId)
        submit.addProperty("source", "jetbrains-plugin")
        submit.addProperty("target", "project-controller")
        submit.addProperty("namespace", "agent-doc")
        submit.addProperty("name", name)
        submit.addProperty("authority_generation", 0)
        submit.addProperty("idempotency_key", idempotencyKey)
        submit.addProperty("deadline_ms", deadlineMs)
        submit.add("policy", JsonObject().also { policy ->
            policy.addProperty("dedupe", "same_idempotency_key")
            policy.addProperty("supersede", supersede)
            policy.addProperty("cancel_on_preempt", true)
        })
        submit.addProperty("payload_type", payloadType)
        submit.addProperty("payload_hash", "sha256:" + sha256Hex(payloadBytes))
        submit.add("payload", JsonObject().also { value ->
            value.add("Inline", JsonArray().also { arr ->
                payloadBytes.forEach { arr.add(it.toInt() and 0xFF) }
            })
        })
        submit.add("required_features", JsonArray().also { arr ->
            arr.add("causal-receipts")
            arr.add("command-events")
        })
        val message = JsonObject()
        message.add("CommandSubmit", submit)

        val request = JsonObject()
        request.addProperty("command", controllerCommand)
        request.addProperty("file", filePath)
        request.addProperty("diagnostic_payload", message.toString())
        return request
    }

    // Resolve a unary `call` from the controller's returned command projection.
    // Terminal-only: an `applied` terminal yields the output; anything else (a
    // non-terminal projection, or a rejected terminal) is a failure result.
internal fun resolveCommandSubmitData(data: JsonObject, commandId: String): CpEditorRouteResult {
    return resolveCommandSubmitTerminalData(data, commandId)
        ?: CpEditorRouteResult(
            1,
            data.get("output")?.asString?.ifEmpty { "command plane returned a non-terminal projection" }
                ?: "command plane returned a non-terminal projection",
        )
}

/** Return a terminal command result, or null while the command is still in flight. */
internal fun resolveCommandSubmitTerminalData(data: JsonObject, commandId: String): CpEditorRouteResult? {
    val output = data.get("output")?.asString ?: ""
    val commands = data.getAsJsonObject("projection")?.getAsJsonArray("commands")
    val entry = commands?.firstOrNull {
        it.isJsonObject && it.asJsonObject.get("command_id")?.asString == commandId
    }?.asJsonObject
        ?: return CpEditorRouteResult(1, output.ifEmpty { "command plane returned no projection entry" })
    if (entry.get("terminal")?.asBoolean != true) {
        return null
    }
        val status = CommandProjectionStatus.fromToken(entry.get("status")?.asString)
        if (status != CommandProjectionStatus.Applied) {
            val reason = entry.get("reason")?.takeIf { !it.isJsonNull }?.asString
            return CpEditorRouteResult(
                1,
                output.ifEmpty {
                    "editor_route ${status?.token ?: CommandProjectionStatus.Rejected.token}: ${reason ?: ""}"
                },
            )
        }
        return CpEditorRouteResult(
            0,
            output,
            steering = parseTurnSteeringAck(data.getAsJsonObject("payload")),
        )
    }

    internal fun resolveCommandSubmitAcceptedData(
        data: JsonObject,
        commandId: String,
        commandName: String,
    ): CpEditorRouteResult {
        val output = data.get("output")?.asString ?: "$commandName accepted"
        val commands = data.getAsJsonObject("projection")?.getAsJsonArray("commands")
        val entry = commands?.firstOrNull {
            it.isJsonObject && it.asJsonObject.get("command_id")?.asString == commandId
        }?.asJsonObject
            ?: return CpEditorRouteResult(1, output.ifEmpty { "command plane returned no projection entry" })
        val status = CommandProjectionStatus.fromToken(entry.get("status")?.asString)
        val terminal = entry.get("terminal")?.asBoolean ?: false
        if (terminal && status != CommandProjectionStatus.Applied) {
            val reason = entry.get("reason")?.takeIf { !it.isJsonNull }?.asString
            return CpEditorRouteResult(
                1,
                output.ifEmpty {
                    "$commandName ${status?.token ?: CommandProjectionStatus.Rejected.token}: ${reason ?: ""}"
                },
            )
        }
        if (
            status == CommandProjectionStatus.Submitted ||
            status == CommandProjectionStatus.Accepted ||
            status == CommandProjectionStatus.Running ||
            status == CommandProjectionStatus.Applied
        ) {
            return CpEditorRouteResult(0, output)
        }
        return CpEditorRouteResult(
            1,
            output.ifEmpty { "$commandName unexpected command status: ${status?.token ?: "<missing>"}" },
        )
    }

    internal fun cpcSocket(projectRoot: String): File = File(projectRoot, ".agent-doc/controller.sock")

    /// `#jbsockdeadline`: hard ceiling on a single controller request.
    ///
    /// This is a HANG GUARD, not a latency control, and the distinction sets the
    /// value. A wedged controller previously blocked the route thread forever
    /// (a blocking `SocketChannel` has no read-timeout API, so `readLine()` never
    /// returns), which also stranded the RUN_AGENT_DOC registry slot so every
    /// later click deduped away — the likely mechanism behind "Run Agent Doc does
    /// nothing".
    ///
    /// It must stay comfortably ABOVE the longest legitimate server-side wait,
    /// which is `routed_cycle_ack_timeout` at 30s with a live child. Setting it
    /// near the client's 15s deadline hint would abort routes that are still
    /// running correctly — the exact failure recorded in #jbroutasync.
private const val SOCKET_REQUEST_TIMEOUT_MS = 60_000L
private const val COMMAND_COMPLETION_GRACE_MS = 5_000L
private const val MIN_COMMAND_AWAIT_TIMEOUT_MS = 1L

    private val socketWatchdog = Executors.newSingleThreadScheduledExecutor { runnable ->
        Thread(runnable, "agent-doc-cp-socket-watchdog").apply { isDaemon = true }
    }

    private fun sendRequestDataToSocket(socket: File, request: JsonObject): JsonObject {
        return sendRequestDataToSocketWithTimeout(socket, request, SOCKET_REQUEST_TIMEOUT_MS)
    }

    private fun sendRequestDataToSocketWithTimeout(
        socket: File,
        request: JsonObject,
        timeoutMs: Long,
    ): JsonObject {
        SocketChannel.open(UnixDomainSocketAddress.of(socket.toPath())).use { channel ->
            // Closing the channel is what unblocks a stuck `readLine()`; there is
            // no per-read timeout to set on a blocking channel.
            val timedOut = java.util.concurrent.atomic.AtomicBoolean(false)
            val watchdog = socketWatchdog.schedule({
                timedOut.set(true)
                try {
                    channel.close()
                } catch (_: Exception) { /* already closing */ }
            }, timeoutMs, TimeUnit.MILLISECONDS)
            try {
                return readRequestData(channel, request)
            } catch (e: Exception) {
                if (timedOut.get()) {
                    throw IllegalStateException(
                        "Project Controller did not respond within ${timeoutMs}ms " +
                            "(socket=${socket.path}); the controller may be wedged",
                        e,
                    )
                }
                throw e
            } finally {
                watchdog.cancel(false)
            }
        }
    }

    private fun readRequestData(channel: SocketChannel, request: JsonObject): JsonObject {
        val writer = Channels.newWriter(channel, Charsets.UTF_8)
        writer.write(request.toString())
        writer.write("\n")
        writer.flush()

        val reader = Channels.newReader(channel, Charsets.UTF_8).buffered()
        val line = reader.readLine()
            ?: throw IllegalStateException("Project Controller returned an empty response")
        val root = JsonParser.parseString(line).asJsonObject
        controllerFailureMessageUtil(root)?.let { error ->
            throw IllegalStateException(error)
        }
        return root.get("data")?.takeIf { it.isJsonObject }?.asJsonObject
        ?: throw IllegalStateException("Project Controller response missing data")
    }

    private fun sendToSocket(socket: File, request: JsonObject): CpEditorRouteResult {
        val data = sendRequestDataToSocket(socket, request)
        return CpEditorRouteResult(
            exitCode = data.get("exit_code")?.asInt ?: 1,
            output = data.get("output")?.asString ?: "",
            steering = parseTurnSteeringAck(data),
        )
    }

    private fun parseTurnSteeringAck(routeResult: JsonObject?): CpTurnSteeringAck? {
        val steering = routeResult?.getAsJsonObject("steering") ?: return null
        return CpTurnSteeringAck(
            kind = steering.get("kind")?.asString ?: return null,
            steeringId = steering.get("steering_id")?.asString ?: return null,
            outcome = steering.get("outcome")?.asString ?: return null,
            acceptedBytes = steering.get("accepted_bytes")?.asInt ?: return null,
        )
    }

private fun sendAcceptedCommandSubmitToSocket(
        socket: File,
        request: JsonObject,
        commandId: String,
        commandName: String,
    ): CpEditorRouteResult {
    val data = sendRequestDataToSocket(socket, request)
    return resolveCommandSubmitAcceptedData(data, commandId, commandName)
}

private fun awaitCommandSubmitTerminal(
    socket: File,
    filePath: String,
    commandId: String,
    timeoutMs: Long,
    commandName: String,
): CpEditorRouteResult {
    val data = sendRequestDataToSocketWithTimeout(
        socket,
        editorCommandAwaitRequest(filePath, commandId, timeoutMs),
        if (timeoutMs > Long.MAX_VALUE - COMMAND_COMPLETION_GRACE_MS) {
            Long.MAX_VALUE
        } else {
            timeoutMs + COMMAND_COMPLETION_GRACE_MS
        },
    )
    return resolveCommandSubmitTerminalData(data, commandId)
        ?: CpEditorRouteResult(
            1,
            "$commandName await returned a non-terminal projection",
        )
}
}
