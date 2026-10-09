package com.github.btakita.agentdoc.split.backend

import com.github.btakita.agentdoc.AgentDocSessionFiles
import com.github.btakita.agentdoc.CpRouteClient
import com.github.btakita.agentdoc.controllerFailureMessageUtil
import com.github.btakita.agentdoc.split.FrontendSurfaceSnapshot
import com.google.gson.JsonArray
import com.google.gson.JsonObject
import com.google.gson.JsonParser
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.LocalFileSystem
import java.net.UnixDomainSocketAddress
import java.nio.channels.Channels
import java.nio.channels.SocketChannel
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runInterruptible
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout

internal data class SurfaceDeliveryResult(
    val accepted: Boolean,
    val diagnostic: String? = null,
)

internal interface SurfaceSnapshotPublisher {
    suspend fun publish(
        clientId: String,
        generation: Long,
        snapshot: FrontendSurfaceSnapshot,
    ): SurfaceDeliveryResult

    suspend fun retire(clientId: String, generation: Long)
}

/**
 * Atomic boundary effect from authenticated RPC facts to the controller policy.
 *
 * This deliberately does not fan a complete snapshot out through the legacy
 * `editor_surface_observe` route.  That route applies focus/layout effects per surface and could
 * mutate main before the detached/main policy sees the whole fact.  Controllers without the new
 * atomic command reject it, so this path fails closed until the policy bridge is deployed.
 */
internal class ControllerSurfaceSnapshotPublisher(private val project: Project) : SurfaceSnapshotPublisher {
    override suspend fun publish(
        clientId: String,
        generation: Long,
        snapshot: FrontendSurfaceSnapshot,
    ): SurfaceDeliveryResult {
        val projectRoot = project.basePath
            ?: return SurfaceDeliveryResult(false, "backend project has no base path")
        val wireClientId = wireClientId(clientId)
        return try {
            send(
                projectRoot,
                ControllerSurfaceSnapshotWire.snapshotRequest(
                    wireClientId,
                    generation,
                    snapshot,
                    ::isSessionDocument,
                ),
            )
            SurfaceDeliveryResult(true)
        } catch (failure: Exception) {
            LOG.warn("[split-surface] controller delivery failed", failure)
            SurfaceDeliveryResult(false, failure.message)
        }
    }

    override suspend fun retire(clientId: String, generation: Long) {
        val projectRoot = project.basePath ?: return
        try {
            send(
                projectRoot,
                ControllerSurfaceSnapshotWire.retirementRequest(wireClientId(clientId), generation),
            )
        } catch (failure: Exception) {
            // The retained backend connection is already retired even when the controller is
            // unavailable. Its generation can never publish again; a later complete snapshot is
            // required before a replacement generation can affect layout.
            LOG.warn("[split-surface] controller retirement unavailable", failure)
        }
    }

    private fun isSessionDocument(path: String): Boolean {
        val file = LocalFileSystem.getInstance().findFileByPath(path) ?: return false
        return AgentDocSessionFiles.isSessionDocument(file)
    }

    private suspend fun send(projectRoot: String, request: JsonObject): JsonObject =
        withTimeout(SOCKET_TIMEOUT_MS) {
            withContext(Dispatchers.IO) {
                runInterruptible {
                    val socket = CpRouteClient.cpcSocket(projectRoot)
                    SocketChannel.open(UnixDomainSocketAddress.of(socket.toPath())).use { channel ->
                        val writer = Channels.newWriter(channel, Charsets.UTF_8)
                        writer.write(request.toString())
                        writer.write("\n")
                        writer.flush()
                        val line = Channels.newReader(channel, Charsets.UTF_8).buffered().readLine()
                            ?: error("Project Controller returned an empty response")
                        val root = JsonParser.parseString(line).asJsonObject
                        controllerFailureMessageUtil(root)?.let { error -> throw IllegalStateException(error) }
                        root.getAsJsonObject("data")
                            ?: error("Project Controller response missing data")
                    }
                }
            }
        }

    private fun wireClientId(authenticatedClientId: String): String =
        "jetbrains-rd:$authenticatedClientId"

    companion object {
        private const val SOCKET_TIMEOUT_MS = 2_000L
        private val LOG = Logger.getInstance(ControllerSurfaceSnapshotPublisher::class.java)
    }
}

internal object ControllerSurfaceSnapshotWire {
    fun snapshotRequest(
        wireClientId: String,
        generation: Long,
        snapshot: FrontendSurfaceSnapshot,
        isSessionDocument: (String) -> Boolean,
    ): JsonObject = JsonObject().also { request ->
        request.addProperty("command", "editor_view_snapshot_observe")
        request.addProperty("generation", generation)
        request.addProperty("sequence", snapshot.sequence)
        request.addProperty("caller", wireClientId)
        request.addProperty("reason", "complete_editor_view_snapshot")
        request.addProperty(
            "diagnostic_payload",
            JsonObject().also { payload ->
                payload.addProperty("schema_version", snapshot.schemaVersion)
                payload.addProperty("client_id", wireClientId)
                payload.addProperty("connection_generation", generation)
                payload.addProperty("sequence", snapshot.sequence)
                payload.addProperty("complete", snapshot.complete)
                // The 262 artifact has no proven per-surface terminal host yet. The controller
                // policy must freeze rather than authorizing any legacy/main terminal effect.
                payload.addProperty("terminal_capable", false)
                payload.add(
                    "surfaces",
                    JsonArray().also { surfaces ->
                        snapshot.surfaces.forEach { surface ->
                            val visible = surface.windows
                                .flatMap { it.visiblePaths }
                                .filter(isSessionDocument)
                                .distinct()
                            val focused = if (surface.focused) {
                                surface.windows
                                    .sortedBy { it.ordinal }
                                    .firstNotNullOfOrNull { it.selectedPath?.takeIf(isSessionDocument) }
                                    ?: visible.firstOrNull()
                                    ?: ""
                            } else {
                                ""
                            }
                            surfaces.add(
                                JsonObject().also { encodedSurface ->
                                    encodedSurface.addProperty("surface_id", surface.paneId)
                                    encodedSurface.addProperty("surface_generation", surface.surfaceGeneration)
                                    encodedSurface.addProperty("role", surface.role.name.lowercase())
                                    encodedSurface.addProperty("focused", focused)
                                    encodedSurface.add(
                                        "visible",
                                        paths(visible),
                                    )
                                },
                            )
                        }
                    },
                )
            }.toString(),
        )
    }

    fun retirementRequest(wireClientId: String, generation: Long): JsonObject =
        JsonObject().also { request ->
            request.addProperty("command", "editor_view_client_retire")
            request.addProperty("generation", generation)
            request.addProperty("caller", wireClientId)
            request.addProperty("reason", "editor_view_client_retired")
        }

    private fun paths(values: List<String>): JsonArray =
        JsonArray().also { array -> values.distinct().forEach(array::add) }

}
