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
        val empty = nativeReloadReplicaRestartReport(
            expectedPaths = emptyList(),
            attachedPaths = emptyList(),
            liveProjects = 3,
        )
        assertFalse("0/0 is an observed empty state, not convergence", empty.converged)
        assertEquals(3, empty.liveProjects)
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
            "native restart must rediscover every live project's open agent-doc documents",
            restart.contains("handoff.projectDocuments.keys + liveProjects") &&
                restart.contains("isAgentDocDocumentTextUtil(document.text)"),
        )
        assertTrue(
            "the coordinator must inspect convergence before releasing the reload gate",
            coordinator.contains("if (report.converged)") &&
                coordinator.contains("replica restart incomplete"),
        )
    }

    /**
     * `#steerreplicachurn`: a repeated `reload_library` request for a build that is
     * already loaded must not quiesce anything. Each such request used to
     * deregister and re-register every open document's replica.
     */
    @Test
    fun `reload that would keep the loaded generation is a no-op`() {
        assertTrue(nativeReloadKeepsCurrentGenerationUtil(100L, 100L, 0L))
        assertTrue("an unreadable target never reloads", nativeReloadKeepsCurrentGenerationUtil(100L, 0L, 0L))
        assertTrue("a build that already failed validation is not retried", nativeReloadKeepsCurrentGenerationUtil(100L, 200L, 200L))
        assertFalse(nativeReloadKeepsCurrentGenerationUtil(100L, 200L, 0L))
        assertFalse(nativeReloadKeepsCurrentGenerationUtil(100L, 200L, 150L))
    }

    @Test
    fun `replicas are rebuilt only when the quiesce tore them down`() {
        val tornDown = NativeReloadReplicaHandoff(emptyMap(), reloadSafe = false, replicasTornDown = true)
        val untouched = NativeReloadReplicaHandoff(emptyMap(), reloadSafe = false, replicasTornDown = false)
        assertFalse("watcher quiesce failed first", nativeReloadReplicaRestartRequiredUtil(false, null))
        assertFalse("capture missed its deadline", nativeReloadReplicaRestartRequiredUtil(true, untouched))
        assertTrue(nativeReloadReplicaRestartRequiredUtil(true, tornDown))
        assertTrue("a quiesce that threw is assumed to have disposed", nativeReloadReplicaRestartRequiredUtil(true, null))
    }

    @Test
    fun `coordinator checks for a no-op before quiescing and gates the replica rebuild`() {
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
        val noOp = coordinator.indexOf("AgentDocLib.reloadWouldKeepCurrentGeneration()")
        val quiesce = coordinator.indexOf("CrdtReplicaManager.quiesceAllForNativeReload()")
        assertTrue("the no-op check must precede every quiesce", noOp in 0 until quiesce)
        val gate = coordinator.indexOf("nativeReloadReplicaRestartRequiredUtil(")
        val restart = coordinator.indexOf("CrdtReplicaManager.restartAfterNativeReload(")
        assertTrue("the replica rebuild must be gated", gate in 0 until restart)
        val captureFailure = manager
            .substringAfter("if (captured == null) {")
            .substringBefore("capturedByProject[project] = captured")
        assertTrue(
            "a capture that missed its deadline disposed nothing",
            captureFailure.contains("replicasTornDown = false"),
        )
    }
}
