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
    fun `replica restart retains every path torn down by the handoff`() {
        assertEquals(
            listOf("/project/a.md", "/project/b.md"),
            nativeReloadReplicaRestartPaths(
                handoffPaths = listOf("/project/b.md", "/project/a.md"),
                observedPaths = emptyList(),
            ),
        )
        assertEquals(
            listOf("/project/a.md", "/project/b.md", "/project/c.md"),
            nativeReloadReplicaRestartPaths(
                handoffPaths = listOf("/project/a.md", "/project/b.md"),
                observedPaths = listOf("/project/c.md", "/project/a.md"),
            ),
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
        assertTrue("native restart must report exact per-document failures", restart.contains("NativeReloadReplicaRestartReport"))
        assertTrue(
            "GH #276: the report must recount replicas when it is taken, not trust each awaited answer",
            restart.contains("settledNativeReloadReplicaRestartReport(") &&
                restart.contains("nativeReloadReplicaAttached(path)") &&
                restart.contains("nativeReloadRegistrationConverging(path)"),
        )
        assertTrue(
            "native restart must rediscover every live project's open agent-doc documents",
            restart.contains("handoff.projectDocuments.keys + liveProjects") &&
                restart.contains("EditorOpenFileSurface.snapshot(project)") &&
                restart.contains("nativeReloadReplicaRestartPaths("),
        )
        assertTrue(
            "native restart must retain the exact handoff set and retry unresolved targets",
            restart.contains("handoff.projectDocuments[project].orEmpty()") &&
                restart.contains("native-reload-document-unavailable"),
        )
        assertTrue(
            "the coordinator must inspect convergence before releasing the reload gate",
            coordinator.contains("nativeReloadReplicaRestartLogLineUtil(report)") &&
                coordinator.contains("report.converged ->") &&
                coordinator.contains("replica restart incomplete"),
        )
    }

    /**
     * GH #281: projection recovery can replace an attached forwarder after the
     * reload snapshots the map but before its serialized checkpoint task runs.
     * The replacement is the live authority and must be checkpointed instead of
     * turning a safe reload into a permanent fail-closed state.
     */
    @Test
    fun `native checkpoint follows a live forwarder replacement`() {
        val manager = Files.readString(
            listOf(
                Paths.get("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
                Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
            ).first { Files.exists(it) },
        )
        val capture = manager
            .substringAfter("private fun captureNativeReloadResumeStates(")
            .substringBefore("companion object")

        assertTrue(
            "the serialized checkpoint must resolve the current authoritative forwarder",
            capture.contains("val currentForwarder = forwarders[filePath]"),
        )
        assertTrue(
            "a replacement that is live when the task runs must provide the resume state",
            capture.contains("currentForwarder?.captureResumeState()"),
        )
        assertFalse(
            "stale snapshot identity must not permanently reject a safe reload",
            capture.contains("forwarders[filePath] !== forwarder"),
        )
    }

    /**
     * GH #276, as measured 2026-10-15: four of 30 awaited attaches outlived the 750 ms
     * wait (`not_attached ... retry=scheduled`) and each attached 0.03-3.2 s later,
     * before the report was logged. The report taken at that moment must count them.
     */
    @Test
    fun `a replica that attached after its bounded wait is counted at report time`() {
        val clock = FakeClock()
        val paths = (1..30).map { "/project/doc-%02d.md".format(it) }
        val late = mapOf(
            paths[3] to 33L,
            paths[11] to 3_200L,
            paths[19] to 701L,
            paths[27] to 932L,
        )
        // The awaited pass ended with these four unattached; the report is taken 1 s
        // later, after the last awaited document. Three have attached by then.
        clock.nowMs = 1_000L
        val report = settledNativeReloadReplicaRestartReport(
            expectedPaths = paths,
            isAttached = { path -> late[path]?.let { clock.nowMs >= it } ?: true },
            isPending = { path -> late[path]?.let { clock.nowMs < it } ?: false },
            settleMillis = NATIVE_RELOAD_REPLICA_SETTLE_MS,
            nowNanos = clock::nanos,
            sleepMillis = clock::sleep,
        )

        assertEquals(30, report.expected)
        assertEquals(30, report.attached)
        assertEquals(emptyList<String>(), report.failedPaths)
        assertEquals(emptyList<String>(), report.pendingPaths)
        assertTrue(report.converged)
        assertEquals("the settle ends when the last replica attaches", 3_200L, clock.nowMs)
        assertEquals("[native] replica restart converged attached=30/30", nativeReloadReplicaRestartLogLineUtil(report))
    }

    @Test
    fun `an attach still converging after the settle is pending, not failed`() {
        val clock = FakeClock()
        val report = settledNativeReloadReplicaRestartReport(
            expectedPaths = listOf("/p/a.md", "/p/slow.md", "/p/dead.md"),
            isAttached = { path -> path == "/p/a.md" },
            // slow.md's attach is still in flight; dead.md has nothing left to retry it.
            isPending = { path -> path == "/p/slow.md" },
            settleMillis = 3_000L,
            liveProjects = 2,
            nowNanos = clock::nanos,
            sleepMillis = clock::sleep,
        )

        assertEquals(1, report.attached)
        assertEquals(listOf("/p/slow.md"), report.pendingPaths)
        assertEquals(listOf("/p/dead.md"), report.failedPaths)
        assertEquals(2, report.liveProjects)
        assertFalse(report.converged)
        assertEquals("the settle is bounded by its budget", 3_000L, clock.nowMs)
        assertEquals(
            "[native] replica restart incomplete attached=1/3 failed=/p/dead.md pending=/p/slow.md",
            nativeReloadReplicaRestartLogLineUtil(report),
        )

        val pendingOnly = nativeReloadReplicaRestartReport(
            expectedPaths = listOf("/p/a.md", "/p/slow.md"),
            attachedPaths = listOf("/p/a.md"),
            pendingPaths = listOf("/p/slow.md"),
        )
        assertFalse("a pending replica is not attached yet", pendingOnly.converged)
        assertEquals(emptyList<String>(), pendingOnly.failedPaths)
        assertEquals(
            "[native] replica restart pending attached=1/2 pending=/p/slow.md " +
                "(registration still converging; retries armed)",
            nativeReloadReplicaRestartLogLineUtil(pendingOnly),
        )
        assertEquals("documents=1/2:pending=/p/slow.md", dynamicLoadReattachReceipt(pendingOnly))
    }

    @Test
    fun `the settle does not wait on a path nothing will retry`() {
        val clock = FakeClock()
        val report = settledNativeReloadReplicaRestartReport(
            expectedPaths = listOf("/p/a.md", "/p/dead.md"),
            isAttached = { path -> path == "/p/a.md" },
            isPending = { false },
            settleMillis = 3_000L,
            pollMillis = 25L,
            nowNanos = clock::nanos,
            sleepMillis = clock::sleep,
        )

        assertEquals(listOf("/p/dead.md"), report.failedPaths)
        assertEquals(
            "one confirming poll, not the whole budget, for a path with nothing converging",
            25L,
            clock.nowMs,
        )
    }

    @Test
    fun `a retry handing off between timer and attach is not read as failed`() {
        val clock = FakeClock()
        // The retry timer has fired (no armed task) but its attach is not submitted
        // yet for one poll; it is then in flight and attaches at 200 ms.
        val report = settledNativeReloadReplicaRestartReport(
            expectedPaths = listOf("/p/retry.md"),
            isAttached = { clock.nowMs >= 200L },
            isPending = { clock.nowMs in 25L until 200L },
            settleMillis = 3_000L,
            pollMillis = 25L,
            nowNanos = clock::nanos,
            sleepMillis = clock::sleep,
        )

        assertTrue(report.converged)
        assertEquals(200L, clock.nowMs)
    }

    @Test
    fun `a zero settle budget reports without waiting`() {
        val clock = FakeClock()
        val report = settledNativeReloadReplicaRestartReport(
            expectedPaths = listOf("/p/slow.md"),
            isAttached = { false },
            isPending = { true },
            settleMillis = 0L,
            nowNanos = clock::nanos,
            sleepMillis = clock::sleep,
        )

        assertEquals(listOf("/p/slow.md"), report.pendingPaths)
        assertEquals(0L, clock.nowMs)
    }

    private class FakeClock {
        var nowMs = 0L

        fun nanos(): Long = TimeUnit.MILLISECONDS.toNanos(nowMs)

        fun sleep(ms: Long) {
            nowMs += ms
        }
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
