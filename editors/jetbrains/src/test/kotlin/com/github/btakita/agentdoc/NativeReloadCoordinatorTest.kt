package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.file.Files
import java.nio.file.Paths
import java.util.concurrent.TimeUnit

class NativeReloadCoordinatorTest {
    @Test
    fun `reload gate coalesces handoffs and releases waiting actions`() {
        val gate = NativeReloadGate()
        val handoff = gate.begin()

        assertTrue(handoff != null)
        assertNull(gate.begin())
        assertFalse(gate.awaitReady(1))

        gate.complete(requireNotNull(handoff))

        assertTrue(gate.awaitReady(1))
        assertTrue(gate.begin() != null)
    }

    @Test
    fun `manager waits share one reload deadline`() {
        val deadline = TimeUnit.MILLISECONDS.toNanos(5_000L)

        assertEquals(5_000L, nativeReloadRemainingWaitMillis(deadline, 0L))
        assertEquals(1L, nativeReloadRemainingWaitMillis(deadline, deadline - 1L))
        assertNull(nativeReloadRemainingWaitMillis(deadline, deadline))
        assertNull(nativeReloadRemainingWaitMillis(deadline, deadline + 1L))
    }

    @Test
    fun `replica restart report requires proof for every open document`() {
        val report = nativeReloadReplicaRestartReport(
            expectedPaths = listOf("/project/a.md", "/project/b.md", "/project/a.md"),
            attachedPaths = listOf("/project/a.md", "/project/unrelated.md"),
        )

        assertEquals(2, report.expected)
        assertEquals(1, report.attached)
        assertEquals(listOf("/project/b.md"), report.failedPaths)
        assertFalse(report.converged)
        assertTrue(
            nativeReloadReplicaRestartReport(
                expectedPaths = listOf("/project/a.md"),
                attachedPaths = listOf("/project/a.md"),
            ).converged,
        )
    }

    @Test
    fun `native handoff checkpoints before disposal and awaits replacement registration`() {
        val manager = Files.readString(
            listOf(
                Paths.get("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
                Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
            ).first { Files.exists(it) },
        )
        val coordinator = Files.readString(
            listOf(
                Paths.get("src/main/kotlin/com/github/btakita/agentdoc/NativeReloadCoordinator.kt"),
                Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/NativeReloadCoordinator.kt"),
            ).first { Files.exists(it) },
        )

        val capture = manager.indexOf("captureNativeReloadResumeStates(captureDeadlineNanos)")
        val shutdown = manager.indexOf("manager.documentWorkers.shutdownNow()", capture)
        val dispose = manager.indexOf("manager.dispose()", capture)
        assertTrue("every replica must be checkpointed before worker shutdown", capture >= 0 && shutdown > capture)
        assertTrue("every replica must be checkpointed before manager disposal", dispose > capture)

        val restart = manager
            .substringAfter("internal fun restartAfterNativeReload(")
            .substringBefore("fun requestRemoteDrain(")
        assertTrue("native restart must wait for each registration", restart.contains("await = true"))
        assertTrue("native restart must report exact per-document failures", restart.contains("nativeReloadReplicaRestartReport"))
        assertTrue(
            "the coordinator must inspect convergence before releasing the reload gate",
            coordinator.contains("if (report.converged)") &&
                coordinator.contains("replica restart incomplete"),
        )
    }
}
