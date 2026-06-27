package com.github.btakita.agentdoc

import com.sun.jna.Pointer
import com.sun.jna.ptr.LongByReference
import com.google.gson.JsonObject
import com.google.gson.JsonParser
import java.io.File
import java.net.UnixDomainSocketAddress
import java.nio.channels.Channels
import java.nio.channels.SocketChannel
import java.util.Base64

/**
 * Thin editor-as-replica forwarding seam (`#crdtauth5`, plan phase 3/5).
 *
 * The plugin stays THIN: it owns no CRDT logic. The yrs replica, state-vector
 * logic, and op encode/decode all live once in the shared Rust cdylib
 * (`agent_doc_replica_*`); this seam is just the binding that
 *
 *  1. forwards a local IntelliJ `Document` delta into the FFI replica
 *     (`apply_local`), encodes the resulting update (`diff` against an empty
 *     state vector, i.e. "everything a fresh peer is missing"), and ships it to
 *     the supervisor over the new `replica_update` IPC family, and
 *  2. applies a remote update received from the supervisor back into the FFI
 *     replica (`apply_update`) so a peer's keystrokes converge locally —
 *     cursor/undo preserving is handled by the caller that writes the converged
 *     text back through the IntelliJ Document API.
 *
 * Both the FFI replica binding ([ReplicaNode]) and the supervisor transport
 * ([ReplicaTransport]) are injected so the seam is unit-testable without loading
 * the native library or opening a real Unix-domain socket. The production wiring
* (a [ReplicaNode] backed by [AgentDocLib] + a [ReplicaTransport] backed by the
* supervisor socket + an IntelliJ `DocumentListener`) is the operator's live
* hookup — see [NativeReplicaNode] and [CrdtReplicaManager].
*/
class CrdtReplicaForwarder(
    private val filePath: String,
    private val identity: String,
    private val node: ReplicaNode,
    private val transport: ReplicaTransport,
) {
    /** True once [register] succeeded and the replica is bound. */
    @Volatile
    var attached: Boolean = false
        private set

    var clientId: Long = 0
        private set

    /**
     * Register this editor as a replica with the supervisor hub and open the
     * local FFI replica bootstrapped from the canonical state the supervisor
     * returns. Returns true when the document is editor-attached and the replica
     * is live; false when the supervisor refuses (e.g. a headless / Detached
     * document) — the caller then falls back to the existing patch-file path.
     */
    fun register(): Boolean {
        val ack = transport.register(filePath, identity) ?: return false
        clientId = ack.clientId
        if (!node.open(ack.clientId, ack.bootstrap)) return false
        attached = true
        return true
    }

    /**
     * Forward a local `Document` delta into the replica and broadcast it to the
     * supervisor hub. Offsets/lengths are yrs char units (the caller converts the
     * IntelliJ UTF-16 offsets first). A no-op when not [attached].
     */
    fun forwardLocalDelta(offset: Int, deleteLen: Int, insert: String) {
        if (!attached) return
        if (!node.applyLocal(clientId, offset, deleteLen, insert)) return
        // Encode "everything a fresh peer is missing" — the hub integrates and
        // re-derives the minimal per-peer delta on the other side.
        val update = node.encodeState() ?: return
        transport.broadcastUpdate(filePath, identity, update)
    }

    /**
     * Align a newly attached native replica with the live editor buffer before
     * forwarding the first real `DocumentEvent` delta. The supervisor bootstrap is
     * seeded from disk, while JetBrains can already hold unsaved edits; applying
     * an event offset against that stale bootstrap can otherwise clamp/truncate.
     */
    fun ensureEditorText(editorText: String) {
        if (!attached) return
        val current = node.text() ?: return
        if (current == editorText) return
        val deleteLen = current.codePointCount(0, current.length)
        if (!node.applyLocal(clientId, 0, deleteLen, editorText)) return
        val update = node.encodeState() ?: return
        transport.broadcastUpdate(filePath, identity, update)
    }

    /**
     * Apply a remote update (a peer's ops fanned out by the supervisor hub) into
     * the local replica. Returns the converged text the caller should write back
     * into the IntelliJ Document (cursor/undo preserving), or null on failure /
     * when not attached.
     */
    fun applyRemoteUpdate(update: ByteArray): String? {
        if (!attached) return null
        if (!node.applyUpdate(clientId, update)) return null
        return node.text()
    }

    /** Pull remote updates the supervisor queued for this replica. */
    fun pullRemoteUpdates(): List<ReplicaRemoteUpdate> {
        if (!attached) return emptyList()
        return transport.pullUpdates(filePath, identity)
    }

    /** ACK a remote update after the caller has applied [applyRemoteUpdate]'s text to the editor buffer. */
    fun ackRemoteUpdate(update: ReplicaRemoteUpdate): Boolean {
        if (!attached) return false
        return transport.ackUpdate(filePath, identity, update.patchId, update.generation)
    }

    /** Deregister the replica from the hub and close the local FFI replica. */
    fun deregister() {
        if (!attached) return
        transport.deregister(filePath, identity)
        node.close(clientId)
        attached = false
    }
}

/** The supervisor `register` ack: the minted client-id + canonical bootstrap state. */
data class ReplicaRegisterAck(val clientId: Long, val bootstrap: ByteArray?)

/** One queued supervisor-to-editor CRDT update owned by this replica. */
data class ReplicaRemoteUpdate(
    val patchId: String,
    val origin: Long,
    val target: Long,
    val generation: Long,
    val update: ByteArray,
)

/**
 * Transport to the supervisor's per-document relay hub over the new
 * `#crdtauth5` IPC family (`replica_register` / `replica_deregister` /
 * `replica_update`). Injected so the seam is testable without a real socket.
 */
interface ReplicaTransport {
    /** `replica_register`; null when the supervisor refuses (Detached document). */
    fun register(filePath: String, identity: String): ReplicaRegisterAck?

    /** `replica_update`: ship a local yrs update to the hub for fan-out. */
    fun broadcastUpdate(filePath: String, identity: String, update: ByteArray)

    /** `replica_pull`: fetch pending peer updates queued for this replica. */
    fun pullUpdates(filePath: String, identity: String): List<ReplicaRemoteUpdate> = emptyList()

    /** `replica_ack`: confirm a pulled update has been applied to the local editor. */
    fun ackUpdate(filePath: String, identity: String, patchId: String, generation: Long): Boolean = false

    /** `replica_deregister`. */
    fun deregister(filePath: String, identity: String)
}

/**
 * Production transport over the supervisor's NDJSON Unix-domain socket.
 *
 * The transport is intentionally conservative: it tries the latest supervisor
 * sockets under `.agent-doc/supervisor/` and treats refusal/no socket as a soft
 * detached fallback so the legacy patch-file path remains available.
 */
class SupervisorSocketReplicaTransport(private val projectRoot: String) : ReplicaTransport {
    private val log = com.intellij.openapi.diagnostic.Logger.getInstance(SupervisorSocketReplicaTransport::class.java)
    @Volatile private var cachedSocket: File? = null

    override fun register(filePath: String, identity: String): ReplicaRegisterAck? {
        val response = send(
            jsonRequest("replica_register", filePath, identity),
        ) ?: return null
        if (!response.ok) return null
        val data = response.data ?: return null
        val clientId = data.get("client_id")?.asLong ?: return null
        val bootstrap = data.get("bootstrap_b64")?.asString?.let { decodeBase64(it) }
        return ReplicaRegisterAck(clientId, bootstrap)
    }

    override fun broadcastUpdate(filePath: String, identity: String, update: ByteArray) {
        val request = jsonRequest("replica_update", filePath, identity)
        request.addProperty("update_b64", Base64.getEncoder().encodeToString(update))
        send(request)
    }

    override fun pullUpdates(filePath: String, identity: String): List<ReplicaRemoteUpdate> {
        val response = send(jsonRequest("replica_pull", filePath, identity)) ?: return emptyList()
        if (!response.ok) return emptyList()
        val updates = response.data?.getAsJsonArray("updates") ?: return emptyList()
        return updates.mapNotNull { element ->
            val item = element.asJsonObject
            val patchId = item.get("patch_id")?.asString ?: return@mapNotNull null
            val updateB64 = item.get("update_b64")?.asString ?: return@mapNotNull null
            ReplicaRemoteUpdate(
                patchId = patchId,
                origin = item.get("origin")?.asLong ?: 0L,
                target = item.get("target")?.asLong ?: 0L,
                generation = item.get("generation")?.asLong ?: return@mapNotNull null,
                update = decodeBase64(updateB64) ?: return@mapNotNull null,
            )
        }
    }

    override fun ackUpdate(filePath: String, identity: String, patchId: String, generation: Long): Boolean {
        val request = jsonRequest("replica_ack", filePath, identity)
        request.addProperty("patch_id", patchId)
        request.addProperty("generation", generation)
        val response = send(request) ?: return false
        return response.ok && (response.data?.get("acknowledged")?.asBoolean ?: false)
    }

    override fun deregister(filePath: String, identity: String) {
        send(jsonRequest("replica_deregister", filePath, identity))
    }

    private data class SupervisorResponse(
        val ok: Boolean,
        val data: JsonObject?,
        val error: String?,
    )

    private fun jsonRequest(method: String, filePath: String, identity: String): JsonObject {
        val obj = JsonObject()
        obj.addProperty("method", method)
        obj.addProperty("file", filePath)
        obj.addProperty("identity", identity)
        return obj
    }

    private fun send(request: JsonObject): SupervisorResponse? {
        val candidates = socketCandidates()
        for (socket in candidates) {
            try {
                val response = sendToSocket(socket, request)
                cachedSocket = socket
                return response
            } catch (e: Exception) {
                if (socket == cachedSocket) cachedSocket = null
                log.debug("[crdt-replica] supervisor socket ${socket.path} unavailable: ${e.message}")
            }
        }
        return null
    }

    private fun socketCandidates(): List<File> {
        val cached = cachedSocket?.takeIf { it.exists() }
        val dir = File(projectRoot, ".agent-doc/supervisor")
        val discovered = dir.listFiles { file -> file.extension == "sock" }
            ?.sortedByDescending { it.lastModified() }
            ?: emptyList()
        return if (cached != null) listOf(cached) + discovered.filter { it != cached } else discovered
    }

    private fun sendToSocket(socket: File, request: JsonObject): SupervisorResponse {
        SocketChannel.open(UnixDomainSocketAddress.of(socket.toPath())).use { channel ->
            val writer = Channels.newWriter(channel, Charsets.UTF_8)
            writer.write(request.toString())
            writer.write("\n")
            writer.flush()
            val reader = Channels.newReader(channel, Charsets.UTF_8).buffered()
            val line = reader.readLine() ?: return SupervisorResponse(false, null, "empty response")
            val root = JsonParser.parseString(line).asJsonObject
            val data = root.get("data")?.takeIf { it.isJsonObject }?.asJsonObject
            return SupervisorResponse(
                ok = root.get("ok")?.asBoolean ?: false,
                data = data,
                error = root.get("error")?.asString,
            )
        }
    }

    private fun decodeBase64(value: String): ByteArray? = try {
        Base64.getDecoder().decode(value)
    } catch (e: IllegalArgumentException) {
        log.debug("[crdt-replica] bad base64 update: ${e.message}")
        null
    }
}

/**
 * The local CRDT replica node. The production implementation ([NativeReplicaNode])
 * delegates every call to the shared Rust cdylib so there is NO CRDT logic in
 * Kotlin. Injected so unit tests can substitute an in-memory fake.
 */
interface ReplicaNode {
    fun open(clientId: Long, initState: ByteArray?): Boolean
    fun applyLocal(clientId: Long, offset: Int, deleteLen: Int, insert: String): Boolean
    fun applyUpdate(clientId: Long, update: ByteArray): Boolean
    fun encodeState(): ByteArray?
    fun text(): String?
    fun close(clientId: Long)
}

/**
 * Production [ReplicaNode] backed by the shared cdylib (`agent_doc_replica_*`).
 *
 * This is the FFI-first node: the plugin holds only a [Long] client-id handle and
 * marshals byte buffers; yrs lives entirely in Rust. ABI errors fail soft (the
 * forwarder falls back to the patch-file path).
 */
class NativeReplicaNode : ReplicaNode {
    private val log = com.intellij.openapi.diagnostic.Logger.getInstance(NativeReplicaNode::class.java)

    override fun open(clientId: Long, initState: ByteArray?): Boolean {
        val lib = AgentDocLib.get() ?: return false
        return try {
            val ok = lib.agent_doc_replica_open(clientId, initState, (initState?.size ?: 0).toLong()) == 0
            if (ok) clientIdForEncode = clientId
            ok
        } catch (e: Throwable) {
            log.debug("[native] replica_open unavailable: ${e.message}")
            false
        }
    }

    override fun applyLocal(clientId: Long, offset: Int, deleteLen: Int, insert: String): Boolean {
        val lib = AgentDocLib.get() ?: return false
        return try {
            lib.agent_doc_replica_apply_local(clientId, offset, deleteLen, insert) == 0
        } catch (e: Throwable) {
            log.debug("[native] replica_apply_local failed: ${e.message}")
            false
        }
    }

    override fun applyUpdate(clientId: Long, update: ByteArray): Boolean {
        val lib = AgentDocLib.get() ?: return false
        return try {
            lib.agent_doc_replica_apply_update(clientId, update, update.size.toLong()) == 0
        } catch (e: Throwable) {
            log.debug("[native] replica_apply_update failed: ${e.message}")
            false
        }
    }

    override fun encodeState(): ByteArray? {
        val lib = AgentDocLib.get() ?: return null
        return try {
            val outLen = LongByReference()
            val ptr: Pointer = lib.agent_doc_replica_encode_state(clientIdForEncode, outLen) ?: return null
            try {
                val len = outLen.value.toInt()
                ptr.getByteArray(0, len)
            } finally {
                lib.agent_doc_free_state(ptr, outLen.value)
            }
        } catch (e: Throwable) {
            log.debug("[native] replica_encode_state failed: ${e.message}")
            null
        }
    }

    override fun text(): String? {
        val lib = AgentDocLib.get() ?: return null
        return try {
            val ptr = lib.agent_doc_replica_text(clientIdForEncode) ?: return null
            try {
                ptr.getString(0)
            } finally {
                lib.agent_doc_free_string(ptr)
            }
        } catch (e: Throwable) {
            log.debug("[native] replica_text failed: ${e.message}")
            null
        }
    }

    override fun close(clientId: Long) {
        val lib = AgentDocLib.get() ?: return
        try {
            lib.agent_doc_replica_close(clientId)
        } catch (e: Throwable) {
            log.debug("[native] replica_close failed: ${e.message}")
        }
    }

    // encode_state/text are keyed on the open replica id; the forwarder opens one
    // replica per node instance, so it stamps the active id on open.
    @Volatile
    private var clientIdForEncode: Long = 0
}

/*
 * Production live hookup lives in CrdtReplicaManager: it owns the DocumentListener,
 * supervisor-socket transport, pull/ACK loop, and minimal editor-buffer apply.
 * This file stays the testable seam around the native replica node and transport.
 */
