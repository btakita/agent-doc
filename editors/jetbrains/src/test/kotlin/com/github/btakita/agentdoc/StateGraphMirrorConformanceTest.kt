package com.github.btakita.agentdoc

import com.google.gson.JsonArray
import com.google.gson.JsonObject
import com.google.gson.JsonParser
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.Base64

/**
 * `#lazilystatesync5` / `#6n5j` — cross-editor convergence parity (Kotlin half).
 *
 * The shared canonical input is the lazily-spec conformance fixture pair
 * (`conformance/agent-doc/{snapshot,delta}_agent_doc_state.json`), vendored into
 * this module's test resources byte-identical from
 * `src/lazily-spec/conformance/agent-doc/`. The Rust authoritative graph
 * (`state_wire.rs mod conformance_parity`) and the VS Code mirror
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
 * The fixtures use the lazily-spec *generic graph* wire shape
 * (`node` / `state.Payload` byte arrays / adjacently-tagged `{ "CellSet": … }`).
 * The agent-doc [StateGraphMirror] applies the flattened agent-doc wire shape.
 * [adaptSnapshot] / [adaptDelta] translate the canonical generic ops into the
 * agent-doc mirror format, then the mirror derives the projection and we assert
 * convergence to the canonical expectation.
 */
class StateGraphMirrorConformanceTest {
    private fun loadFixture(name: String): JsonObject {
        val stream = javaClass.classLoader.getResourceAsStream("conformance/agent-doc/$name")
            ?: error("conformance fixture not on test classpath: $name")
        val raw = stream.bufferedReader(Charsets.UTF_8).use { it.readText() }
        return JsonParser.parseString(raw).asJsonObject
    }

    /** lazily-spec `state.Payload` / `payload.Inline` byte arrays → base64 string. */
    private fun bytesToBase64(arr: JsonArray): String {
        val bytes = ByteArray(arr.size()) { arr[it].asInt.toByte() }
        return Base64.getEncoder().encodeToString(bytes)
    }

    /** Translate the generic-graph snapshot fixture into the agent-doc mirror wire. */
    private fun adaptSnapshot(fixture: JsonObject): String {
        val wire = fixture.getAsJsonObject("wire").getAsJsonObject("Snapshot")
        val epoch = wire.get("epoch").asLong
        val nodes = StringBuilder()
        wire.getAsJsonArray("nodes").forEach { element ->
            val node = element.asJsonObject
            if (nodes.isNotEmpty()) nodes.append(',')
            val slot = node.get("node").asLong
            val typeTag = node.get("type_tag").asString
            val payload = bytesToBase64(node.getAsJsonObject("state").getAsJsonArray("Payload"))
            nodes.append("""{"slot_id":$slot,"type_tag":"$typeTag","state":"resolved","payload":"$payload"}""")
        }
        val edges = StringBuilder()
        wire.getAsJsonArray("edges").forEach { element ->
            val edge = element.asJsonObject
            if (edges.isNotEmpty()) edges.append(',')
            edges.append("""{"dependent":${edge.get("dependent").asLong},"dependency":${edge.get("dependency").asLong}}""")
        }
        return """{"type":"snapshot","epoch":$epoch,"document_hash":"parity-doc","nodes":[$nodes],"edges":[$edges],"roots":[]}"""
    }

    /** Translate the generic-graph delta fixture into the agent-doc mirror wire. */
    private fun adaptDelta(fixture: JsonObject): String {
        val wire = fixture.getAsJsonObject("wire").getAsJsonObject("Delta")
        val baseEpoch = wire.get("base_epoch").asLong
        val epoch = wire.get("epoch").asLong
        val ops = StringBuilder()
        wire.getAsJsonArray("ops").forEach { element ->
            val op = element.asJsonObject
            val key = op.keySet().first()
            val body = op.getAsJsonObject(key)
            if (ops.isNotEmpty()) ops.append(',')
            when (key) {
                "CellSet" -> {
                    val payload = bytesToBase64(body.getAsJsonObject("payload").getAsJsonArray("Inline"))
                    ops.append("""{"op":"cell_set","slot_id":${body.get("node").asLong},"payload":"$payload"}""")
                }
                "SlotValue" -> {
                    val payload = bytesToBase64(body.getAsJsonObject("payload").getAsJsonArray("Inline"))
                    ops.append("""{"op":"slot_value","slot_id":${body.get("node").asLong},"payload":"$payload"}""")
                }
                "NodeAdd" -> {
                    val payload = bytesToBase64(body.getAsJsonObject("state").getAsJsonArray("Payload"))
                    ops.append(
                        """{"op":"node_add","slot_id":${body.get("node").asLong},""" +
                            """"type_tag":"${body.get("type_tag").asString}","payload":"$payload"}""",
                    )
                }
                "NodeRemove" -> ops.append("""{"op":"node_remove","slot_id":${body.get("node").asLong}}""")
                "EdgeAdd" -> ops.append(
                    """{"op":"edge_add","dependent":${body.get("dependent").asLong},"dependency":${body.get("dependency").asLong}}""",
                )
                "EdgeRemove" -> ops.append(
                    """{"op":"edge_remove","dependent":${body.get("dependent").asLong},"dependency":${body.get("dependency").asLong}}""",
                )
                "Invalidate" -> ops.append("""{"op":"invalidate","slot_id":${body.get("node").asLong}}""")
                else -> error("unknown generic-graph op: $key")
            }
        }
        return """{"type":"delta","base_epoch":$baseEpoch,"epoch":$epoch,"document_hash":"parity-doc","ops":[$ops]}"""
    }

    private fun phaseOf(mirror: StateGraphMirror, typeTag: String): String? =
        mirror.payloadObject(typeTag)
            ?.get("phase")
            ?.takeIf { it.isJsonPrimitive }
            ?.asString

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
    fun `kt mirror applying canonical snapshot then delta converges to the pinned expectation`() {
        val snapshotFixture = loadFixture("snapshot_agent_doc_state.json")
        val deltaFixture = loadFixture("delta_agent_doc_state.json")

        val mirror = StateGraphMirror()
        assertTrue(mirror.applyMessage(adaptSnapshot(snapshotFixture)))

        // Snapshot-time canonical phases (preflight_started / selected).
        assertEquals(3L, mirror.epoch)
        assertEquals("preflight_started", phaseOf(mirror, AgentDocNodeType.CLOSEOUT_CYCLE))
        assertEquals("selected", phaseOf(mirror, AgentDocNodeType.QUEUE_HEAD))

        // Apply the warm delta — the mirror must converge to the after-state.
        assertTrue(mirror.applyMessage(adaptDelta(deltaFixture)))
        assertEquals(6L, mirror.epoch)
        assertEquals("committed", phaseOf(mirror, AgentDocNodeType.CLOSEOUT_CYCLE))
        assertEquals("completed", phaseOf(mirror, AgentDocNodeType.QUEUE_HEAD))

        // Transport patch added mid-cycle, phase applied — readable via the
        // reactive summary the editor consumes (route/transport/proof slice).
        val summary = MirrorProjectionSummary.fromMirror(mirror)
        assertNotNull(summary)
        assertEquals("applied", summary.latestTransportPhase)
        assertEquals(
            1,
            mirror.nodesOfType(AgentDocNodeType.TRANSPORT_PATCH).size,
        )
    }

    @Test
    fun `kt mirror reapplying the canonical delta is idempotent`() {
        val mirror = StateGraphMirror()
        mirror.applyMessage(adaptSnapshot(loadFixture("snapshot_agent_doc_state.json")))
        val delta = adaptDelta(loadFixture("delta_agent_doc_state.json"))
        mirror.applyMessage(delta)
        val afterFirst = MirrorProjectionSummary.fromMirror(mirror).compact()
        val epochAfterFirst = mirror.epoch
        val nodesAfterFirst = mirror.nodeCount

        // Re-emit the SAME delta — the pure-fold property means a replay is a
        // no-op: epoch frontier holds, node set + derived summary unchanged.
        mirror.applyMessage(delta)
        assertEquals(epochAfterFirst, mirror.epoch)
        assertEquals(nodesAfterFirst, mirror.nodeCount)
        assertEquals(afterFirst, MirrorProjectionSummary.fromMirror(mirror).compact())
        assertEquals("committed", phaseOf(mirror, AgentDocNodeType.CLOSEOUT_CYCLE))
    }
}
