package com.github.btakita.agentdoc

import com.google.gson.JsonArray
import com.google.gson.JsonObject
import com.google.gson.JsonParser
import com.intellij.openapi.diagnostic.Logger
import java.io.File
import java.net.UnixDomainSocketAddress
import java.nio.channels.Channels
import java.nio.channels.SocketChannel

internal data class CpcEditorRouteResult(
    val exitCode: Int,
    val output: String,
)

internal data class CpcTmuxLayoutSyncState(
    val synced: Boolean,
    val reason: String,
)

/**
 * High-level editor route RPC over the CPC/project-controller socket.
 */
internal object CpcRouteClient {
    private val log = Logger.getInstance(CpcRouteClient::class.java)

    fun runEditorRoute(
        projectRoot: String,
        filePath: String,
        relativePath: String,
        layoutArgs: List<String>,
        waitForReadySeconds: Long,
        attemptId: String?,
        routeKey: String?,
    ): CpcEditorRouteResult {
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
            )
            return try {
                sendCommandSubmitToSocket(socket, request, commandId)
            } catch (e: Exception) {
                log.warn("[route] command-plane editor_route request failed via ${socket.path}: ${e.message}")
                CpcEditorRouteResult(
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
        )
        return try {
            sendToSocket(socket, request)
        } catch (e: Exception) {
            log.warn("[route] CPC editor_route request failed via ${socket.path}: ${e.message}")
            CpcEditorRouteResult(
                exitCode = 1,
                output = "CPC editor_route request failed via ${socket.path}: ${e.message}",
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
    ): CpcEditorRouteResult {
        val socket = cpcSocket(projectRoot)
        val commandId = "cmd-" + java.util.UUID.randomUUID().toString()
        val request = syncTmuxLayoutCommandSubmitRequest(
            projectRoot = projectRoot,
            columnsJson = columnsJson,
            window = window,
            focus = focus,
            noAutostart = noAutostart,
            exactVisible = exactVisible,
            commandId = commandId,
            callerKind = callerKind,
            controllerCommand = "editor_command_submit_async",
        )
        return try {
            sendAcceptedCommandSubmitToSocket(socket, request, commandId, "sync_tmux_layout")
        } catch (e: Exception) {
            log.warn("[sync] command-plane sync_tmux_layout submit failed via ${socket.path}: ${e.message}")
            CpcEditorRouteResult(
                exitCode = 1,
                output = "command-plane sync_tmux_layout submit failed via ${socket.path}: ${e.message}",
            )
        }
    }

    fun submitFocusDocumentPane(
        projectRoot: String,
        documentPath: String,
    ): CpcEditorRouteResult {
        val socket = cpcSocket(projectRoot)
        val commandId = "cmd-" + java.util.UUID.randomUUID().toString()
        val request = focusDocumentPaneCommandSubmitRequest(
            projectRoot = projectRoot,
            documentPath = documentPath,
            commandId = commandId,
            controllerCommand = "editor_command_submit_async",
        )
        return try {
            sendAcceptedCommandSubmitToSocket(socket, request, commandId, "focus_document_pane")
        } catch (e: Exception) {
            log.warn("[focus] command-plane focus_document_pane submit failed via ${socket.path}: ${e.message}")
            CpcEditorRouteResult(
                exitCode = 1,
                output = "command-plane focus_document_pane submit failed via ${socket.path}: ${e.message}",
            )
        }
    }

    fun tmuxLayoutSyncState(
        projectRoot: String,
        columnsJson: String,
        focus: String?,
    ): CpcTmuxLayoutSyncState? {
        val socket = cpcSocket(projectRoot)
        val request = tmuxLayoutSyncStateRequest(columnsJson, focus)
        return try {
            val data = sendRequestDataToSocket(socket, request)
            CpcTmuxLayoutSyncState(
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

    // The `agent-doc.editor_route.v1` payload the controller consumes, shared by
    // the classic `editor_route` request and the command-plane submit.
    internal fun editorRoutePayload(
        relativePath: String,
        layoutArgs: List<String>,
        waitForReadySeconds: Long,
        attemptId: String?,
        routeKey: String?,
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
        return payload
    }

    internal fun editorRouteRequest(
        filePath: String,
        relativePath: String,
        layoutArgs: List<String>,
        waitForReadySeconds: Long,
        attemptId: String?,
        routeKey: String?,
    ): JsonObject {
        val payload = editorRoutePayload(relativePath, layoutArgs, waitForReadySeconds, attemptId, routeKey)
        val request = JsonObject()
        request.addProperty("command", "editor_route")
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
    ): JsonObject {
        val payload = editorRoutePayload(relativePath, layoutArgs, waitForReadySeconds, attemptId, routeKey)
        return commandSubmitRequest(
            filePath = filePath,
            name = "editor_route",
            payloadType = "agent-doc.editor_route.v1",
            payload = payload,
            idempotencyKey = routeKey ?: relativePath,
            commandId = commandId,
            deadlineMs = waitForReadySeconds * 1000,
            supersede = false,
        )
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
            name = "sync_tmux_layout",
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
        return commandSubmitRequest(
            filePath = documentPath,
            name = "focus_document_pane",
            payloadType = "agent-doc.focus_document_pane.v1",
            payload = payload,
            idempotencyKey = "$projectRoot:$documentPath:focus",
            commandId = commandId,
            deadlineMs = 2_000,
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
    internal fun resolveCommandSubmitData(data: JsonObject, commandId: String): CpcEditorRouteResult {
        val output = data.get("output")?.asString ?: ""
        val commands = data.getAsJsonObject("projection")?.getAsJsonArray("commands")
        val entry = commands?.firstOrNull {
            it.isJsonObject && it.asJsonObject.get("command_id")?.asString == commandId
        }?.asJsonObject
        if (entry == null || entry.get("terminal")?.asBoolean != true) {
            return CpcEditorRouteResult(1, output.ifEmpty { "command plane returned a non-terminal projection" })
        }
        val status = entry.get("status")?.asString
        if (status != "applied") {
            val reason = entry.get("reason")?.takeIf { !it.isJsonNull }?.asString
            return CpcEditorRouteResult(1, output.ifEmpty { "editor_route ${status ?: "rejected"}: ${reason ?: ""}" })
        }
        return CpcEditorRouteResult(0, output)
    }

    internal fun resolveCommandSubmitAcceptedData(
        data: JsonObject,
        commandId: String,
        commandName: String,
    ): CpcEditorRouteResult {
        val output = data.get("output")?.asString ?: "$commandName accepted"
        val commands = data.getAsJsonObject("projection")?.getAsJsonArray("commands")
        val entry = commands?.firstOrNull {
            it.isJsonObject && it.asJsonObject.get("command_id")?.asString == commandId
        }?.asJsonObject
            ?: return CpcEditorRouteResult(1, output.ifEmpty { "command plane returned no projection entry" })
        val status = entry.get("status")?.asString
        val terminal = entry.get("terminal")?.asBoolean ?: false
        if (terminal && status != "applied") {
            val reason = entry.get("reason")?.takeIf { !it.isJsonNull }?.asString
            return CpcEditorRouteResult(1, output.ifEmpty { "$commandName ${status ?: "rejected"}: ${reason ?: ""}" })
        }
        if (status == "submitted" || status == "accepted" || status == "running" || status == "applied") {
            return CpcEditorRouteResult(0, output)
        }
        return CpcEditorRouteResult(1, output.ifEmpty { "$commandName unexpected command status: ${status ?: "<missing>"}" })
    }

    internal fun cpcSocket(projectRoot: String): File = File(projectRoot, ".agent-doc/controller.sock")

    private fun sendRequestDataToSocket(socket: File, request: JsonObject): JsonObject {
        SocketChannel.open(UnixDomainSocketAddress.of(socket.toPath())).use { channel ->
            val writer = Channels.newWriter(channel, Charsets.UTF_8)
            writer.write(request.toString())
            writer.write("\n")
            writer.flush()

            val reader = Channels.newReader(channel, Charsets.UTF_8).buffered()
            val line = reader.readLine()
                ?: throw IllegalStateException("CPC returned an empty response")
            val root = JsonParser.parseString(line).asJsonObject
            if (root.get("ok")?.asBoolean != true) {
                throw IllegalStateException(root.get("error")?.asString ?: "CPC request failed")
            }
            return root.get("data")?.takeIf { it.isJsonObject }?.asJsonObject
                ?: throw IllegalStateException("CPC response missing data")
        }
    }

    private fun sendToSocket(socket: File, request: JsonObject): CpcEditorRouteResult {
        val data = sendRequestDataToSocket(socket, request)
        return CpcEditorRouteResult(
            exitCode = data.get("exit_code")?.asInt ?: 1,
            output = data.get("output")?.asString ?: "",
        )
    }

    private fun sendCommandSubmitToSocket(
        socket: File,
        request: JsonObject,
        commandId: String,
    ): CpcEditorRouteResult {
        val data = sendRequestDataToSocket(socket, request)
        return resolveCommandSubmitData(data, commandId)
    }

    private fun sendAcceptedCommandSubmitToSocket(
        socket: File,
        request: JsonObject,
        commandId: String,
        commandName: String,
    ): CpcEditorRouteResult {
        val data = sendRequestDataToSocket(socket, request)
        return resolveCommandSubmitAcceptedData(data, commandId, commandName)
    }
}
