package com.github.btakita.agentdoc

import com.google.gson.JsonObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** `#jbshrinkrestarts`: the keystroke benchmark's fixture and gate stay comparable across runs. */
class KeystrokeLatencyBenchmarkTest {
    @Test
    fun `the synthetic fixture is a deterministic large session document with an operator anchor`() {
        val document = KeystrokeBenchmark.syntheticSessionDocument()
        assertEquals("the fixture must be byte-identical across runs", document, KeystrokeBenchmark.syntheticSessionDocument())
        val bytes = document.toByteArray(Charsets.UTF_8).size
        assertTrue("about 120 KB, like a long-lived live session: $bytes", bytes in 120 * 1024..140 * 1024)
        assertTrue("multi-byte text exercises the UTF-16/code-point conversion", bytes > document.length)
        assertTrue(isAgentDocDocumentTextUtil(document))
        val anchor = KeystrokeBenchmark.typingAnchor(document)
        assertTrue(document.startsWith("<!-- agent:boundary:", anchor))
    }

    @Test
    fun `typing at the anchor prepares a burst the replica flush can apply`() {
        val document = KeystrokeBenchmark.syntheticSessionDocument()
        val anchor = KeystrokeBenchmark.typingAnchor(document)
        val batch =
            prepareLocalEditorEditsUtil(
                document,
                listOf(
                    CapturedLocalEditorEdit(anchor, "", "x", 0L, document.length),
                    CapturedLocalEditorEdit(anchor, "x", "", 0L, document.length + 1),
                ),
            )
        assertEquals(document, batch?.resultingText)
    }

    private fun report(vararg cases: Pair<String, Double>): JsonObject =
        JsonObject().apply {
            add(
                "cases",
                JsonObject().apply {
                    cases.forEach { (name, p50) ->
                        add(name, JsonObject().apply { addProperty("p50_best", p50); addProperty("p99", p50 * 2) })
                    }
                },
            )
        }

    @Test
    fun `the gate fails only a keystroke case that slowed past its tolerance`() {
        val baseline = report("keystroke.typing_report" to 100.0, "move.route_outcome.native" to 10.0)
        assertTrue(KeystrokeBenchmark.compare(baseline, report("keystroke.typing_report" to 114.0)).passed)
        assertFalse(KeystrokeBenchmark.compare(baseline, report("keystroke.typing_report" to 116.0)).passed)
        // Small cases get an absolute allowance so timer noise cannot fail the gate.
        val tiny = report("keystroke.typing_report" to 2.0)
        assertTrue(KeystrokeBenchmark.compare(tiny, report("keystroke.typing_report" to 6.5)).passed)
        // A non-keystroke case, or a step of a gated case, is reported, never gated.
        assertTrue(KeystrokeBenchmark.compare(baseline, report("move.route_outcome.native" to 500.0)).passed)
        val step = report("keystroke.replica_flush_1.diff" to 100.0)
        assertTrue(KeystrokeBenchmark.compare(step, report("keystroke.replica_flush_1.diff" to 900.0)).passed)
    }
}
