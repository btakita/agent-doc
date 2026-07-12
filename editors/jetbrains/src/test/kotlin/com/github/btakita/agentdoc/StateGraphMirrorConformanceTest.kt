package com.github.btakita.agentdoc

import com.google.gson.JsonObject
import com.google.gson.JsonParser
import io.github.lazily.GraphView
import io.github.lazily.IpcMessage
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Test

/**
 * `#lzsync` 3B — cross-editor convergence parity (Kotlin half), native wire.
 *
 * The shared canonical input is the lazily-spec conformance fixture pair
 * (`conformance/agent-doc/{snapshot,delta}_agent_doc_state.json`), vendored into
 * this module's test resources byte-identical from
 * `src/lazily-spec/conformance/agent-doc/`. The Rust authoritative graph
 * (`state_wire.rs mod conformance_parity`) and the VS Code view
 * (`stateMirrorConformance.test.ts`) assert the SAME expectation against the SAME
 * fixtures, so the three implementations are pinned to one canonical answer
 * without a live cross-language harness:
 *
 * | field | snapshot | after delta |
 * |---|---|---|
 * | `cycle_phase` | `preflight_started` | `committed` |
 * | `queue_head_phase` | `selected` | `completed` |
 * | `epoch` | 3 | 6 |
 * | transport patch phase | (absent) | `applied` |
 *
 * The fixtures already carry the lazily-spec *generic graph* wire shape
 * (`node` / `state.Payload` byte arrays / externally-tagged `{ "Snapshot": … }` /
 * `{ "CellSet": … }`), which is exactly the native `IpcMessage` JSON. The clean
 * split (`#lzsync` 3B) means the generic [GraphView] folds them DIRECTLY — no
 * adaptation to a bespoke agent-doc wire — and agent-doc's [AgentDocProjection] /
 * [AgentDocTurnProjection] layer the domain read on top.
 */
class StateGraphMirrorConformanceTest {
    private fun loadFixture(name: String): JsonObject {
        val stream = javaClass.classLoader.getResourceAsStream("conformance/agent-doc/$name")
            ?: error("conformance fixture not on test classpath: $name")
        val raw = stream.bufferedReader(Charsets.UTF_8).use { it.readText() }
        return JsonParser.parseString(raw).asJsonObject
    }

    /** The fixture's `wire` object is already the native externally-tagged IpcMessage JSON. */
    private fun loadMessage(name: String): IpcMessage =
        IpcMessage.decodeJson(loadFixture(name).getAsJsonObject("wire").toString())

    private fun applyTo(view: GraphView, message: IpcMessage) {
        when (message) {
            is IpcMessage.SnapshotMessage -> view.applySnapshot(message.snapshot)
            is IpcMessage.DeltaMessage -> view.applyDelta(message.delta)
            else -> error("unexpected conformance message: $message")
        }
    }

    /** Read the `phase` field of the single node of [typeTag], or null. */
    private fun phaseOf(view: GraphView, typeTag: String): String? {
        val bytes = view.singletonNode(typeTag)?.payload ?: return null
        return JsonParser.parseString(String(bytes, Charsets.UTF_8))
            .asJsonObject
            .get("phase")
            ?.takeIf { it.isJsonPrimitive }
            ?.asString
    }

    @Test
    fun `fixtures declare the canonical cross-language expectation`() {
        val snapshot = loadFixture("snapshot_agent_doc_state.json").getAsJsonObject("assertions")
        assertEquals(3L, snapshot.get("epoch").asLong)
        assertEquals("preflight_started", snapshot.get("cycle_phase").asString)
        assertEquals("selected", snapshot.get("queue_head_phase").asString)

        val delta = loadFixture("delta_agent_doc_state.json").getAsJsonObject("assertions")
        assertEquals(3L, delta.get("base_epoch").asLong)
        assertEquals(6L, delta.get("epoch").asLong)
        assertEquals("committed", delta.get("cycle_phase_after").asString)
        assertEquals("completed", delta.get("queue_head_phase_after").asString)
        assertEquals(
            "agent_doc.transport.patch",
            delta.getAsJsonArray("added_type_tags").first().asString,
        )
    }

    @Test
    fun `kt view applying canonical snapshot then delta converges to the pinned expectation`() {
        val view = GraphView()
        applyTo(view, loadMessage("snapshot_agent_doc_state.json"))

        // Snapshot-time canonical phases (preflight_started / selected).
        assertEquals(3L, view.epoch)
        assertEquals("preflight_started", phaseOf(view, AgentDocNodeType.CLOSEOUT_CYCLE))
        assertEquals("selected", phaseOf(view, AgentDocNodeType.QUEUE_HEAD))

        // Apply the warm delta — the view must converge to the after-state.
        applyTo(view, loadMessage("delta_agent_doc_state.json"))
        assertEquals(6L, view.epoch)
        assertEquals("committed", phaseOf(view, AgentDocNodeType.CLOSEOUT_CYCLE))
        assertEquals("completed", phaseOf(view, AgentDocNodeType.QUEUE_HEAD))

        // Transport patch added mid-cycle, phase applied — readable via the
        // domain projection the editor consumes (route/transport/proof slice).
        val projection = AgentDocProjection.fromView(view)
        assertNotNull(projection)
        assertEquals("applied", projection.latestTransportPhase)
        assertEquals(1, view.nodesOfType(AgentDocNodeType.TRANSPORT_PATCH).size)
    }

    @Test
    fun `kt view reapplying the canonical delta is idempotent`() {
        val view = GraphView()
        applyTo(view, loadMessage("snapshot_agent_doc_state.json"))
        val delta = loadMessage("delta_agent_doc_state.json")
        applyTo(view, delta)
        val afterFirst = AgentDocProjection.fromView(view).compact()
        val epochAfterFirst = view.epoch
        val nodesAfterFirst = view.nodeCount

        // Re-emit the SAME delta — the pure-fold property means a replay is a
        // no-op: epoch frontier holds, node set + derived projection unchanged.
        applyTo(view, delta)
        assertEquals(epochAfterFirst, view.epoch)
        assertEquals(nodesAfterFirst, view.nodeCount)
        assertEquals(afterFirst, AgentDocProjection.fromView(view).compact())
        assertEquals("committed", phaseOf(view, AgentDocNodeType.CLOSEOUT_CYCLE))
    }
}
