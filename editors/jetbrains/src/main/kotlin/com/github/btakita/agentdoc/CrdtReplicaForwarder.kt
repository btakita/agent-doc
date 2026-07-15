package com.github.btakita.agentdoc

import com.sun.jna.Pointer
import com.sun.jna.ptr.LongByReference
import com.google.gson.JsonObject
import com.google.gson.JsonParser
import java.io.File
import java.net.UnixDomainSocketAddress
import java.nio.channels.Channels
import java.nio.channels.SocketChannel
import java.nio.charset.StandardCharsets
import java.security.MessageDigest
import java.util.Base64
import java.util.concurrent.TimeUnit

/**
 * Thin editor-as-replica forwarding seam (`#crdtauth5`, plan phase 3/5).
 *
 * The plugin stays THIN: it owns no CRDT logic. The lazily replica, state-vector
 * logic, and op encode/decode all live once in the shared Rust cdylib
 * (`agent_doc_replica_*`); this seam is just the binding that
 *
 *  1. forwards a local IntelliJ `Document` delta into the FFI replica
 *     (`apply_local`), encodes the resulting update (`diff` against an empty
 *     state vector, i.e. "everything a fresh peer is missing"), and ships it to
 *     the CPC over the `crdt_replica` RPC envelope, and
 *  2. applies a remote update received from the CPC back into the FFI
 *     replica (`apply_update`) so a peer's keystrokes converge locally —
 *     cursor/undo preserving is handled by the caller that writes the converged
 *     text back through the IntelliJ Document API.
 *
 * Both the FFI replica binding ([ReplicaNode]) and the CPC transport
 * ([ReplicaTransport]) are injected so the seam is unit-testable without loading
 * the native library or opening a real Unix-domain socket. The production wiring
* (a [ReplicaNode] backed by [AgentDocLib] + a [ReplicaTransport] backed by the
* CPC socket + an IntelliJ `DocumentListener`) is the operator's live
* hookup — see [NativeReplicaNode] and [CrdtReplicaManager].
*/
class CrdtReplicaForwarder(
    private val filePath: String,
    private val identity: String,
    private val node: ReplicaNode,
    private val transport: ReplicaTransport,
) {
    private val log = com.intellij.openapi.diagnostic.Logger.getInstance(CrdtReplicaForwarder::class.java)
    private var pushedVersion: ByteArray? = null

    /** True once [register] succeeded and the replica is bound. */
    @Volatile
    var attached: Boolean = false
        private set

    var clientId: Long = 0
        private set

    /**
     * Register this editor as a replica with the CPC-owned document model and open the
     * local FFI replica bootstrapped from the canonical state the CPC
     * returns. Returns true when the document is editor-attached and the replica
     * is live; false when the CPC refuses (e.g. a headless / Detached
     * document) — the caller must keep recovery controller-owned instead of
     * treating editor projections as authority.
     */
    fun register(): Boolean {
        val started = System.nanoTime()
        try {
            val registerStarted = System.nanoTime()
            val ack = transport.register(filePath, identity).also {
                logSlow("transport.register", registerStarted, details = "ok=${it != null}")
            }
            if (ack == null) {
                // NEVER swallow the register cause: without this the failure lands as a
                // bare `[crdt-replica] register failed` WARN with no reason, so a live
                // wedge (controller.sock down / protocol mismatch / oversized bootstrap
                // round-trip) is undiagnosable from idea.log. The transport already
                // recorded the specific `send` failure via `lastError`.
                log.warn("[crdt-replica] transport.register returned null for ${File(filePath).name}; reason=${transport.lastRegisterError() ?: "unknown"}")
                return false
            }
            clientId = ack.clientId
            val openStarted = System.nanoTime()
            val opened = node.open(ack.clientId, ack.bootstrap)
            logSlow("native.open", openStarted, details = "bootstrap_bytes=${ack.bootstrap?.size ?: 0} ok=$opened")
            if (!opened) {
                log.warn("[crdt-replica] native.open rejected the bootstrap for ${File(filePath).name}; bootstrap_bytes=${ack.bootstrap?.size ?: 0} (see prior [native] replica_open WARN for the cause)")
                transport.deregister(filePath, identity)
                return false
            }
            attached = true
            pushedVersion = node.stateVector()
            return true
        } finally {
            logSlow("register", started, warnMs = 100)
        }
    }

    /**
     * Forward a local `Document` delta into the replica and broadcast it to the
     * document model. Offsets/lengths are yrs char units (the caller converts the
     * IntelliJ UTF-16 offsets first). A no-op when not [attached].
     */
    fun forwardLocalDelta(offset: Int, deleteLen: Int, insert: String) {
        if (!attached) return
        val started = System.nanoTime()
        val applyStarted = System.nanoTime()
        if (!node.applyLocal(clientId, offset, deleteLen, insert)) return
        logSlow("native.applyLocal", applyStarted, details = "offset=$offset delete_cp=$deleteLen insert_chars=${insert.length}")
        val updateBytes = publishIncremental("local-delta")
        logSlow("forwardLocalDelta", started, warnMs = 100, details = "update_bytes=$updateBytes")
    }

    /**
     * Align a newly attached native replica with the live editor buffer before
     * forwarding the first real `DocumentEvent` delta. The CPC bootstrap is
     * seeded from disk, while JetBrains can already hold unsaved edits; applying
     * an event offset against that stale bootstrap can otherwise clamp/truncate.
     */
    fun ensureEditorText(editorText: String) {
        if (!attached) return
        val started = System.nanoTime()
        val textStarted = System.nanoTime()
        val current = node.text() ?: return
        logSlow("native.text", textStarted, details = "reason=ensureEditorText chars=${editorText.length}")
        if (current == editorText) return
        val deleteLen = current.codePointCount(0, current.length)
        val applyStarted = System.nanoTime()
        if (!node.applyLocal(clientId, 0, deleteLen, editorText)) return
        logSlow("native.applyLocal", applyStarted, details = "reason=ensureEditorText delete_cp=$deleteLen insert_chars=${editorText.length}")
        // Incremental from the bootstrap frontier. The incident-causing full-state
        // adopt remains disabled; bounded text adopt is triggered only by an explicit
        // controller reattach signal.
        val updateBytes = publishIncremental("ensure-editor-text")
        logSlow("ensureEditorText", started, warnMs = 100, details = "chars=${editorText.length} update_bytes=$updateBytes")
    }

    private fun publishIncremental(reason: String): Int {
        val frontier = pushedVersion ?: node.stateVector() ?: return 0
        val diffStarted = System.nanoTime()
        val update = node.diff(frontier) ?: return 0
        logSlow("native.diff", diffStarted, details = "reason=$reason update_bytes=${update.size}")
        val durable = transport.pushDocumentOps(filePath, update.toString(Charsets.UTF_8))
        if (durable) pushedVersion = node.stateVector()
        val broadcastStarted = System.nanoTime()
        transport.broadcastUpdate(filePath, identity, update)
        logSlow("transport.broadcastUpdate", broadcastStarted, details = "reason=$reason update_bytes=${update.size}")
        return update.size
    }

    /** Genuine-reattach recovery only: bounded authoritative text, never full op-log. */
    fun pushTextAdopt(editorText: String): Boolean = transport.pushTextAdopt(filePath, editorText)

    /**
     * Apply a remote update (a peer's ops fanned out by the document model) into
     * the local replica. Returns the converged text the caller should write back
     * into the IntelliJ Document (cursor/undo preserving), or null on failure /
     * when not attached.
     */
    fun applyRemoteUpdate(update: ByteArray): String? {
        if (!attached) return null
        val started = System.nanoTime()
        val applyStarted = System.nanoTime()
        if (!node.applyUpdate(clientId, update)) return null
        pushedVersion = node.stateVector()
        logSlow("native.applyUpdate", applyStarted, details = "update_bytes=${update.size}")
        val textStarted = System.nanoTime()
        val text = node.text()
        logSlow("native.text", textStarted, details = "reason=applyRemoteUpdate chars=${text?.length ?: -1}")
        logSlow("applyRemoteUpdate", started, warnMs = 100, details = "update_bytes=${update.size} chars=${text?.length ?: -1}")
        return text
    }

    /** Current local replica text, used only as a precondition check before
     * applying CPC-delivered updates. The editor buffer remains authoritative;
     * a mismatch makes the manager publish the editor buffer before mutating
     * this replica further. */
    fun replicaText(): String? {
        if (!attached) return null
        val started = System.nanoTime()
        return node.text().also { text ->
            logSlow("native.text", started, details = "reason=replicaText chars=${text?.length ?: -1}")
        }
    }

    /** Pull remote updates the document model queued for this replica. */
    fun pullRemoteUpdates(): List<ReplicaRemoteUpdate> {
        if (!attached) return emptyList()
        return transport.pullUpdates(filePath, identity)
    }

    /**
     * Pull the pending delivery (D2): normal deltas, or a replace delivery whose
     * text the caller may install only after proving the editor buffer still
     * matches the expected replica baseline.
     */
    fun pullRemoteDelivery(): ReplicaPullDelivery {
        if (!attached) return ReplicaPullDelivery.Deltas(emptyList())
        val started = System.nanoTime()
        val delivery = transport.pullDelivery(filePath, identity)
        val detail = when (delivery) {
            is ReplicaPullDelivery.Deltas -> "updates=${delivery.updates.size}"
            is ReplicaPullDelivery.Replace -> "replace_chars=${delivery.text.length}"
            is ReplicaPullDelivery.Unavailable -> "unavailable=${delivery.reason}"
        }
        logSlow("transport.pullDelivery", started, details = detail)
        return delivery
    }

    /**
     * ACK only after the caller has applied [applyRemoteUpdate]'s text to the
     * visible editor buffer. The content hash turns the ACK into convergence
     * proof; a generation alone proves only that a frame was handled.
     */
    fun ackRemoteUpdate(update: ReplicaRemoteUpdate, appliedText: String?): Boolean {
        if (!attached) return false
        val started = System.nanoTime()
        val appliedContentHash = appliedText?.let(::sha256Text)
        return transport.ackUpdate(
            filePath,
            identity,
            update.patchId,
            update.generation,
            appliedContentHash,
        ).also {
            logSlow(
                "transport.ackUpdate",
                started,
                details = "patch_id=${update.patchId} generation=${update.generation} expected_hash=${update.expectedContentHash ?: "legacy"} applied_hash=${appliedContentHash ?: "unavailable"} ok=$it",
            )
        }
    }

    /** Deregister the replica from the hub and close the local FFI replica. */
    fun deregister() {
        if (!attached) return
        val deregisterStarted = System.nanoTime()
        transport.deregister(filePath, identity)
        logSlow("transport.deregister", deregisterStarted)
        retireLocal()
    }

    /**
     * Close only this process's native node. Used after an atomic register/swap:
     * the replacement is already a live hub member, so retiring the previous
     * connection must not emit another document-close signal.
     */
    fun retireLocal() {
        if (!attached) return
        val closeStarted = System.nanoTime()
        node.close(clientId)
        pushedVersion = null
        logSlow("native.close", closeStarted)
        attached = false
    }

    private fun logSlow(
        operation: String,
        startedNanos: Long,
        warnMs: Long = 50,
        details: String = "",
    ) {
        val elapsedMs = TimeUnit.NANOSECONDS.toMillis(System.nanoTime() - startedNanos)
        val suffix = if (details.isBlank()) "" else " $details"
        val message = "[crdt-perf] $operation file=${File(filePath).name} elapsed_ms=$elapsedMs thread=${Thread.currentThread().name}$suffix"
        if (elapsedMs >= warnMs) {
            log.warn(message)
        } else {
            log.debug(message)
        }
    }
}

/** The CPC `register` ack: the minted client-id + canonical bootstrap state. */
data class ReplicaRegisterAck(val clientId: Long, val bootstrap: ByteArray?)

/** One queued CPC-to-editor CRDT update owned by this replica. */
data class ReplicaRemoteUpdate(
    val patchId: String,
    val origin: Long,
    val target: Long,
    val generation: Long,
    val expectedContentHash: String? = null,
    val update: ByteArray,
)

private fun sha256Text(text: String): String =
    MessageDigest.getInstance("SHA-256")
        .digest(text.toByteArray(StandardCharsets.UTF_8))
        .joinToString("") { "%02x".format(it.toInt() and 0xff) }

/**
 * The outcome of a `replica_pull` (D2). The CPC decides which kind to send
 * (FFI-first): a normal additive-delta delivery, or a **replace** delivery when the
 * editor was flagged for re-bootstrap — an out-of-band *deletion* (`RebuiltFromDisk`)
 * cannot be expressed as an additive CRDT delta, so the plugin may replace its
 * whole buffer with the corrected canonical text only after the editor-buffer
 * baseline check passes.
 */
sealed interface ReplicaPullDelivery {
    data class Deltas(val updates: List<ReplicaRemoteUpdate>) : ReplicaPullDelivery
    data class Replace(val text: String) : ReplicaPullDelivery
    /** The controller transport or prior registration is no longer usable. */
    data class Unavailable(val reason: String) : ReplicaPullDelivery
}

/**
 * Transport to the CPC-owned document model. Injected so the seam is testable
 * without a real socket.
 */
interface ReplicaTransport {
    /** `replica_register`; null when the CPC refuses (Detached document). */
    fun register(filePath: String, identity: String): ReplicaRegisterAck?

    /**
     * Human-readable reason the most recent [register] returned null (socket
     * unavailable, `ok=false`, missing `client_id`, …), or null if unknown. Lets the
     * caller log WHY register failed instead of a bare "register failed" WARN.
     */
    fun lastRegisterError(): String? = null

    /** `replica_update`: ship a local lazily delta to the document model for fan-out. */
    fun broadcastUpdate(filePath: String, identity: String, update: ByteArray)

    /** Durable document-op plane; default success keeps test transports thin. */
    fun pushDocumentOps(filePath: String, deltaJson: String): Boolean = true

    /** Bounded reattach-only adopt; default false means unsupported by a test transport. */
    fun pushTextAdopt(filePath: String, text: String): Boolean = false

    /** Retry the retained document-op suffix. */
    fun flushDocumentOps(filePath: String) {}

    /** `replica_pull`: fetch pending peer updates queued for this replica. */
    fun pullUpdates(filePath: String, identity: String): List<ReplicaRemoteUpdate> = emptyList()

    /**
     * `replica_pull` (D2): fetch the pending delivery, distinguishing a normal
     * additive-delta batch from a replace delivery (out-of-band deletion
     * re-bootstrap). Defaults to wrapping [pullUpdates] so legacy transports/tests
     * keep working.
     */
    fun pullDelivery(filePath: String, identity: String): ReplicaPullDelivery =
        ReplicaPullDelivery.Deltas(pullUpdates(filePath, identity))

    /** `replica_ack`: confirm a pulled update has been applied to the local editor. */
    fun ackUpdate(
        filePath: String,
        identity: String,
        patchId: String,
        generation: Long,
        contentHash: String? = null,
    ): Boolean = false

    /** `replica_deregister`. */
    fun deregister(filePath: String, identity: String)
}

/**
 * Production transport over the CPC/project-controller NDJSON Unix-domain socket.
 */
class CpcSocketReplicaTransport(private val projectRoot: String) : ReplicaTransport {
    private val log = com.intellij.openapi.diagnostic.Logger.getInstance(CpcSocketReplicaTransport::class.java)

    @Volatile
    private var lastRegisterError: String? = null

    override fun lastRegisterError(): String? = lastRegisterError

    override fun register(filePath: String, identity: String): ReplicaRegisterAck? {
        val response = send(
            controllerRequest("replica_register", filePath, identity),
        )
        if (response == null) {
            lastRegisterError = "socket_unavailable: $lastSendError"
            return null
        }
        if (!response.ok) {
            lastRegisterError = "controller_error: ${response.error ?: "ok=false"}"
            return null
        }
        val data = response.data
        if (data == null) {
            lastRegisterError = "missing_data"
            return null
        }
        val clientId = data.get("client_id")?.asLong
        if (clientId == null) {
            val refusalReason = data.get("reason")?.asString
            lastRegisterError = refusalReason?.let { "controller_refused: $it" } ?: "missing_client_id"
            return null
        }
        val bootstrap = data.get("bootstrap_b64")?.asString?.let { decodeBase64(it) }
        lastRegisterError = null
        return ReplicaRegisterAck(clientId, bootstrap)
    }

    override fun broadcastUpdate(filePath: String, identity: String, update: ByteArray) {
        val request = controllerRequest("replica_update", filePath, identity) {
            it.addProperty("update_b64", Base64.getEncoder().encodeToString(update))
        }
        send(request)
    }

    override fun pushDocumentOps(filePath: String, deltaJson: String): Boolean {
        val lib = AgentDocLib.get() ?: return false
        return try {
            lib.agent_doc_reliable_sync_document_op_push(projectRoot, filePath, deltaJson) == 0
        } catch (e: Throwable) {
            log.warn("[document-op] durable push failed for ${File(filePath).name}: ${e.message}")
            false
        }
    }

    override fun pushTextAdopt(filePath: String, text: String): Boolean {
        val lib = AgentDocLib.get() ?: return false
        return try {
            lib.agent_doc_reliable_sync_text_adopt_push(projectRoot, filePath, text) == 0
        } catch (e: Throwable) {
            log.warn("[reattach-adopt] bounded text adopt failed for ${File(filePath).name}: ${e.message}")
            false
        }
    }

    override fun flushDocumentOps(filePath: String) {
        val lib = AgentDocLib.get() ?: return
        try {
            lib.agent_doc_reliable_sync_document_op_flush(projectRoot, filePath)
        } catch (e: Throwable) {
            log.debug("[document-op] retained flush failed: ${e.message}")
        }
    }

    override fun pullDelivery(filePath: String, identity: String): ReplicaPullDelivery {
        flushDocumentOps(filePath)
        val response = send(controllerRequest("replica_pull", filePath, identity))
            ?: return ReplicaPullDelivery.Unavailable(lastSendError ?: "controller_socket_unavailable")
        if (!response.ok) {
            return ReplicaPullDelivery.Unavailable(response.error ?: "controller_rejected_replica_pull")
        }
        // D2: a replace delivery carries corrected canonical text; the manager
        // performs the editor-buffer baseline check before installing it.
        if (response.data?.get("kind")?.asString == "replace") {
            val text = response.data.get("replace")?.asString
            if (text != null) return ReplicaPullDelivery.Replace(text)
        }
        return ReplicaPullDelivery.Deltas(parseUpdates(response.data))
    }

    override fun pullUpdates(filePath: String, identity: String): List<ReplicaRemoteUpdate> {
        val response = send(controllerRequest("replica_pull", filePath, identity)) ?: return emptyList()
        if (!response.ok) return emptyList()
        return parseUpdates(response.data)
    }

    private fun parseUpdates(data: JsonObject?): List<ReplicaRemoteUpdate> {
        val updates = data?.getAsJsonArray("updates") ?: return emptyList()
        return updates.mapNotNull { element ->
            val item = element.asJsonObject
            val patchId = item.get("patch_id")?.asString ?: return@mapNotNull null
            val updateB64 = item.get("update_b64")?.asString ?: return@mapNotNull null
            ReplicaRemoteUpdate(
                patchId = patchId,
                origin = item.get("origin")?.asLong ?: 0L,
                target = item.get("target")?.asLong ?: 0L,
                generation = item.get("generation")?.asLong ?: return@mapNotNull null,
                expectedContentHash = item.get("expected_content_hash")?.asString,
                update = decodeBase64(updateB64) ?: return@mapNotNull null,
            )
        }
    }

    override fun ackUpdate(
        filePath: String,
        identity: String,
        patchId: String,
        generation: Long,
        contentHash: String?,
    ): Boolean {
        val request = controllerRequest("replica_ack", filePath, identity) {
            it.addProperty("patch_id", patchId)
            it.addProperty("generation", generation)
            if (contentHash != null) it.addProperty("content_hash", contentHash)
        }
        val response = send(request) ?: return false
        return response.ok && (response.data?.get("acknowledged")?.asBoolean ?: false)
    }

    override fun deregister(filePath: String, identity: String) {
        send(controllerRequest("replica_deregister", filePath, identity))
    }

    private data class CpcResponse(
        val ok: Boolean,
        val data: JsonObject?,
        val error: String?,
    )

    private fun controllerRequest(
        method: String,
        filePath: String,
        identity: String,
        payloadFields: (JsonObject) -> Unit = {},
    ): JsonObject {
        val payload = JsonObject()
        payload.addProperty("method", method)
        payload.addProperty("identity", identity)
        payload.addProperty("source", "jetbrains_plugin")
        payloadFields(payload)

        val obj = JsonObject()
        obj.addProperty("command", "crdt_replica")
        obj.addProperty("file", filePath)
        obj.addProperty("diagnostic_payload", payload.toString())
        return obj
    }

    @Volatile
    private var lastSendError: String? = null

    private fun send(request: JsonObject): CpcResponse? {
        val socket = cpcSocket()
        return try {
            val response = sendToSocket(socket, request)
            lastSendError = null
            response
        } catch (e: Exception) {
            lastSendError = "${socket.path}: ${e.javaClass.simpleName}: ${e.message}"
            log.debug("[crdt-replica] CPC socket ${socket.path} unavailable: ${e.message}")
            null
        }
    }

    private fun cpcSocket(): File = File(projectRoot, ".agent-doc/controller.sock")

    private fun sendToSocket(socket: File, request: JsonObject): CpcResponse {
        SocketChannel.open(UnixDomainSocketAddress.of(socket.toPath())).use { channel ->
            val writer = Channels.newWriter(channel, Charsets.UTF_8)
            writer.write(request.toString())
            writer.write("\n")
            writer.flush()
            val reader = Channels.newReader(channel, Charsets.UTF_8).buffered()
            val line = reader.readLine() ?: return CpcResponse(false, null, "empty response")
            val root = JsonParser.parseString(line).asJsonObject
            val data = root.get("data")?.takeIf { it.isJsonObject }?.asJsonObject
            return CpcResponse(
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
    fun stateVector(): ByteArray? = byteArrayOf()
    fun diff(theirStateVector: ByteArray): ByteArray? = encodeState()
    fun text(): String?
    fun close(clientId: Long)
}

/**
 * Production [ReplicaNode] backed by the shared cdylib (`agent_doc_replica_*`).
 *
 * This is the FFI-first node: the plugin holds only a [Long] client-id handle and
 * marshals byte buffers; yrs lives entirely in Rust. ABI errors fail soft (the
 * forwarder leaves recovery with the CPC rather than creating a separate
 * editor-authoritative write path).
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
            // Register hinges on this native open; a swallowed DEBUG line makes a
            // real ABI/bootstrap failure invisible in idea.log. Surface it.
            log.warn("[native] replica_open failed clientId=$clientId bootstrap_bytes=${initState?.size ?: 0}: ${e.javaClass.simpleName}: ${e.message}")
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

    override fun stateVector(): ByteArray? {
        val lib = AgentDocLib.get() ?: return null
        return try {
            val outLen = LongByReference()
            val ptr = lib.agent_doc_replica_state_vector(clientIdForEncode, outLen) ?: return null
            try {
                ptr.getByteArray(0, outLen.value.toInt())
            } finally {
                lib.agent_doc_free_state(ptr, outLen.value)
            }
        } catch (e: Throwable) {
            log.debug("[native] replica_state_vector failed: ${e.message}")
            null
        }
    }

    override fun diff(theirStateVector: ByteArray): ByteArray? {
        val lib = AgentDocLib.get() ?: return null
        return try {
            val outLen = LongByReference()
            val ptr = lib.agent_doc_replica_diff(
                clientIdForEncode,
                theirStateVector,
                theirStateVector.size.toLong(),
                outLen,
            ) ?: return null
            try {
                ptr.getByteArray(0, outLen.value.toInt())
            } finally {
                lib.agent_doc_free_state(ptr, outLen.value)
            }
        } catch (e: Throwable) {
            log.debug("[native] replica_diff failed: ${e.message}")
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
 * CPC socket transport, pull/ACK loop, and minimal editor-buffer apply.
 * This file stays the testable seam around the native replica node and transport.
 */
