package com.github.btakita.agentdoc

import io.github.lazily.ThreadSafeContext
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class MergeOwnershipStateChartTest {
    private fun chartAt(phase: MergeOwnershipPhase): MergeOwnershipStateChart {
        val chart = MergeOwnershipStateChart(ThreadSafeContext())
        val path =
            when (phase) {
                MergeOwnershipPhase.Detached -> emptyList()
                MergeOwnershipPhase.Attached ->
                    listOf(MergeOwnershipEvent.EditorAttached)
                MergeOwnershipPhase.EditorOwnsBuffer ->
                    listOf(
                        MergeOwnershipEvent.EditorAttached,
                        MergeOwnershipEvent.EditorBufferObserved,
                    )
                MergeOwnershipPhase.BinaryWriteRequested ->
                    listOf(
                        MergeOwnershipEvent.EditorAttached,
                        MergeOwnershipEvent.EditorBufferObserved,
                        MergeOwnershipEvent.BinaryWriteRequested,
                    )
                MergeOwnershipPhase.LazilyPatchAppliedProven ->
                    listOf(
                        MergeOwnershipEvent.EditorAttached,
                        MergeOwnershipEvent.EditorBufferObserved,
                        MergeOwnershipEvent.BinaryWriteRequested,
                        MergeOwnershipEvent.LazilyPatchAppliedObserved,
                    )
                MergeOwnershipPhase.Committed ->
                    listOf(MergeOwnershipEvent.Committed)
            }
        path.forEach { assertTrue(chart.send(it)) }
        assertEquals(phase, chart.phase)
        return chart
    }

    @Test
    fun `happy path matches binary merge ownership vocabulary`() {
        val chart = MergeOwnershipStateChart(ThreadSafeContext())

        assertEquals(MergeOwnershipPhase.Detached, chart.phase)
        assertFalse(chart.editorAttached)

        assertTrue(chart.send(MergeOwnershipEvent.EditorAttached))
        assertEquals(MergeOwnershipPhase.Attached, chart.phase)
        assertTrue(chart.editorAttached)

        assertTrue(chart.send(MergeOwnershipEvent.EditorBufferObserved))
        assertEquals(MergeOwnershipPhase.EditorOwnsBuffer, chart.phase)

        assertTrue(chart.send(MergeOwnershipEvent.BinaryWriteRequested))
        assertEquals(MergeOwnershipPhase.BinaryWriteRequested, chart.phase)

        assertTrue(chart.send(MergeOwnershipEvent.LazilyPatchAppliedObserved))
        assertEquals(MergeOwnershipPhase.LazilyPatchAppliedProven, chart.phase)

        assertTrue(chart.send(MergeOwnershipEvent.Committed))
        assertEquals(MergeOwnershipPhase.Committed, chart.phase)
        assertFalse(chart.editorAttached)
    }

    @Test
    fun `invalid ownership edges fail closed`() {
        val chart = MergeOwnershipStateChart(ThreadSafeContext())

        assertFalse(chart.send(MergeOwnershipEvent.BinaryWriteRequested))
        assertEquals(MergeOwnershipPhase.Detached, chart.phase)

        assertTrue(chart.send(MergeOwnershipEvent.EditorAttached))
        assertFalse(chart.send(MergeOwnershipEvent.Committed))
        assertEquals(MergeOwnershipPhase.Attached, chart.phase)

        assertTrue(chart.send(MergeOwnershipEvent.EditorBufferObserved))
        assertFalse(chart.send(MergeOwnershipEvent.HeartbeatStale))
        assertEquals(MergeOwnershipPhase.EditorOwnsBuffer, chart.phase)
    }

    @Test
    fun `transition matrix stays byte-for-byte aligned with binary ownership rules`() {
        val expected =
            mapOf(
                MergeOwnershipPhase.Detached to
                    mapOf(
                        MergeOwnershipEvent.EditorAttached to MergeOwnershipPhase.Attached,
                        MergeOwnershipEvent.EditorBufferObserved to MergeOwnershipPhase.EditorOwnsBuffer,
                        MergeOwnershipEvent.EditorDetached to MergeOwnershipPhase.Detached,
                        MergeOwnershipEvent.Committed to MergeOwnershipPhase.Committed,
                    ),
                MergeOwnershipPhase.Attached to
                    mapOf(
                        MergeOwnershipEvent.EditorAttached to MergeOwnershipPhase.Attached,
                        MergeOwnershipEvent.EditorBufferObserved to MergeOwnershipPhase.EditorOwnsBuffer,
                        MergeOwnershipEvent.EditorDetached to MergeOwnershipPhase.Detached,
                        MergeOwnershipEvent.HeartbeatStale to MergeOwnershipPhase.Detached,
                    ),
                MergeOwnershipPhase.EditorOwnsBuffer to
                    mapOf(
                        MergeOwnershipEvent.EditorAttached to MergeOwnershipPhase.Attached,
                        MergeOwnershipEvent.EditorBufferObserved to MergeOwnershipPhase.EditorOwnsBuffer,
                        MergeOwnershipEvent.EditorDetached to MergeOwnershipPhase.Detached,
                        MergeOwnershipEvent.BinaryWriteRequested to MergeOwnershipPhase.BinaryWriteRequested,
                    ),
                MergeOwnershipPhase.BinaryWriteRequested to
                    mapOf(
                        MergeOwnershipEvent.BinaryWriteRequested to MergeOwnershipPhase.BinaryWriteRequested,
                        MergeOwnershipEvent.LazilyPatchAppliedObserved to MergeOwnershipPhase.LazilyPatchAppliedProven,
                    ),
                MergeOwnershipPhase.LazilyPatchAppliedProven to
                    mapOf(
                        MergeOwnershipEvent.LazilyPatchAppliedObserved to MergeOwnershipPhase.LazilyPatchAppliedProven,
                        MergeOwnershipEvent.Committed to MergeOwnershipPhase.Committed,
                    ),
                MergeOwnershipPhase.Committed to
                    mapOf(MergeOwnershipEvent.Committed to MergeOwnershipPhase.Committed),
            )

        for (phase in MergeOwnershipPhase.entries) {
            for (event in MergeOwnershipEvent.entries) {
                val chart = chartAt(phase)
                val next = expected.getValue(phase)[event]
                assertEquals(
                    "$phase + $event acceptance",
                    next != null,
                    chart.send(event),
                )
                assertEquals("$phase + $event state", next ?: phase, chart.phase)
            }
        }
    }

    @Test
    fun `attach observation and detach remain safe across sharing threads`() {
        val context = ThreadSafeContext()
        val chart = MergeOwnershipStateChart(context)
        val attached = CountDownLatch(1)
        val read = CountDownLatch(1)
        val pool = Executors.newFixedThreadPool(2)

        try {
            val writer =
                pool.submit {
                    assertTrue(chart.send(MergeOwnershipEvent.EditorAttached))
                    assertTrue(chart.send(MergeOwnershipEvent.EditorBufferObserved))
                    attached.countDown()
                    assertTrue(read.await(5, TimeUnit.SECONDS))
                    assertTrue(chart.send(MergeOwnershipEvent.EditorDetached))
                }
            val reader =
                pool.submit {
                    assertTrue(attached.await(5, TimeUnit.SECONDS))
                    assertEquals(MergeOwnershipPhase.EditorOwnsBuffer, chart.phase)
                    assertTrue(chart.editorAttached)
                    read.countDown()
                }
            writer.get(5, TimeUnit.SECONDS)
            reader.get(5, TimeUnit.SECONDS)
        } finally {
            pool.shutdown()
            assertTrue(pool.awaitTermination(5, TimeUnit.SECONDS))
        }

        assertEquals(MergeOwnershipPhase.Detached, chart.phase)
        assertFalse(chart.editorAttached)
    }
}
