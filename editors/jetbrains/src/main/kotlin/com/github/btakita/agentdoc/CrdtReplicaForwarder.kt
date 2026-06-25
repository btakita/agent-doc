package com.github.btakita.agentdoc

import com.sun.jna.Pointer
import com.sun.jna.ptr.LongByReference

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
 * hookup — see [NativeReplicaNode] and the doc comment at the bottom.
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

    private var clientId: Long = 0

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

    /** `replica_deregister`. */
    fun deregister(filePath: String, identity: String)
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
 * OPERATOR LIVE HOOKUP (not wired here — see the report):
 *
 * Two thin pieces remain for the live multi-editor repro, both pure plumbing
 * around this tested seam (no new CRDT logic):
 *
 *  1. A `DocumentListener` on the agent-doc VirtualFile's `Document` that, on
 *     each `documentChanged` event, converts the IntelliJ UTF-16 offset/length to
 *     yrs char units and calls `forwardLocalDelta(...)`. (Mirror the existing
 *     `agent_doc_record_editor_op` UTF-16→char conversion already used by the
 *     op-replay reporter.)
 *
 *  2. A Kotlin supervisor-socket client implementing [ReplicaTransport] that
 *     writes the `{"method":"replica_register|replica_update|replica_deregister",
 *     ...}` NDJSON to `.agent-doc/supervisor/<session>.sock` and, on the inbound
 *     side, delivers each fanned-out `update_b64` from the ack `targets` (and any
 *     server-pushed peer updates) to `applyRemoteUpdate(...)`, then writes the
 *     returned converged text back through the Document API inside a
 *     write-action (preserving cursor/undo).
 *
 * The supervisor side of (2) is fully wired and end-to-end tested in Rust
 * (`crdtauth5_end_to_end_fan_out_over_the_ipc_path`).
 */
