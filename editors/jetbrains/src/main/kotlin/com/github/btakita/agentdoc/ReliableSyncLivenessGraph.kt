package com.github.btakita.agentdoc

import com.google.gson.JsonArray
import com.google.gson.JsonObject
import io.github.lazily.OrSet
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap

/**
 * Plugin-side lazily reactive liveness graph (sidecar-retirement Phase 3C,
 * design B).
 *
 * Each editor process holds THIS editor's open-set as lazily-kt [OrSet]s — the
 * same convergent add-wins cell type the controller's `LivenessProjection` folds
 * — one per open document, keyed by the canonical `document_hash`. Because it is
 * a real lazily graph (not opaque FFI state), the plugin UI can react to its own
 * liveness ([isOpen]); and [open] / [close] derive the exact externally-tagged
 * `LivenessOp` batch JSON the plugin pushes through the FFI. The Rust FFI keeps
 * the durable outbox + the controller socket; this graph is the reactive source
 * of truth for what the plugin reports (mirrors S5's `StateGraphMirror` for the
 * reverse, controller→plugin, direction).
 *
 * A re-open mints a fresh presence tag, so it wins over a concurrent lagging
 * close that only observed the earlier tag (OR-set add-wins).
 */
class ReliableSyncLivenessGraph(private val pid: Long) {
    private class DocState {
        val orSet = OrSet()
        val tags = mutableListOf<String>()
    }

    private val docs = ConcurrentHashMap<String, DocState>()

    /**
     * Mark `documentHash` opened by this editor; returns the externally-tagged
     * `Open` `LivenessOp` batch JSON to push.
     */
    @Synchronized
    fun open(documentHash: String): String {
        val tag = UUID.randomUUID().toString()
        val state = docs.getOrPut(documentHash) { DocState() }
        state.orSet.add(tag)
        state.tags.add(tag)
        return opsJson(openOp(documentHash, tag))
    }

    /**
     * Mark `documentHash` closed, observing every tag this editor added; returns
     * the `Close` `LivenessOp` batch JSON, or `null` if it was never opened here.
     */
    @Synchronized
    fun close(documentHash: String): String? {
        val state = docs[documentHash] ?: return null
        val observed = state.tags.toList()
        state.orSet.removeObserved(observed)
        state.tags.clear()
        return opsJson(closeOp(documentHash, observed))
    }

    /** Reactive: is this editor currently holding `documentHash` open? */
    @Synchronized
    fun isOpen(documentHash: String): Boolean = docs[documentHash]?.orSet?.present() == true

    private fun openOp(documentHash: String, tag: String): JsonObject {
        val inner = JsonObject().apply {
            addProperty("document_hash", documentHash)
            addProperty("pid", pid)
            addProperty("tag", tag)
        }
        return JsonObject().apply { add("Open", inner) }
    }

    private fun closeOp(documentHash: String, observed: List<String>): JsonObject {
        val tags = JsonArray().apply { observed.forEach { add(it) } }
        val inner = JsonObject().apply {
            addProperty("document_hash", documentHash)
            addProperty("pid", pid)
            add("observed_tags", tags)
        }
        return JsonObject().apply { add("Close", inner) }
    }

    private fun opsJson(op: JsonObject): String = JsonArray().apply { add(op) }.toString()
}
