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

    internal fun editorRouteRequest(
        filePath: String,
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

        val request = JsonObject()
        request.addProperty("command", "editor_route")
        request.addProperty("file", filePath)
        request.addProperty("diagnostic_payload", payload.toString())
        return request
    }

    internal fun cpcSocket(projectRoot: String): File = File(projectRoot, ".agent-doc/controller.sock")

    private fun sendToSocket(socket: File, request: JsonObject): CpcEditorRouteResult {
        SocketChannel.open(UnixDomainSocketAddress.of(socket.toPath())).use { channel ->
            val writer = Channels.newWriter(channel, Charsets.UTF_8)
            writer.write(request.toString())
            writer.write("\n")
            writer.flush()

            val reader = Channels.newReader(channel, Charsets.UTF_8).buffered()
            val line = reader.readLine()
                ?: return CpcEditorRouteResult(1, "CPC editor_route returned an empty response")
            val root = JsonParser.parseString(line).asJsonObject
            if (root.get("ok")?.asBoolean != true) {
                return CpcEditorRouteResult(
                    exitCode = 1,
                    output = root.get("error")?.asString ?: "CPC editor_route failed",
                )
            }
            val data = root.get("data")?.takeIf { it.isJsonObject }?.asJsonObject
                ?: return CpcEditorRouteResult(1, "CPC editor_route response missing data")
            return CpcEditorRouteResult(
                exitCode = data.get("exit_code")?.asInt ?: 1,
                output = data.get("output")?.asString ?: "",
            )
        }
    }
}
