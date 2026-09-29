package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.file.Files
import java.nio.file.Paths

/**
 * `#rundocdispatchrobust`: after the 2026-09-29 18:12:38 dynamic reload the replacement
 * generation's liveness report never reached the src/haiven-dev controller, which then refused
 * every registration as `replica_register_stale_editor_endpoint`, while the retired generation
 * kept retrying under its old identity. Run Agent Doc on devops.md was refused.
 */
class RunDocDispatchRobustTest {
    private fun source(name: String): String =
        Files.readString(
            listOf(
                Paths.get("src/main/kotlin/com/github/btakita/agentdoc/$name"),
                Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/$name"),
            ).first { Files.exists(it) },
        )

    @Test
    fun `a stale-endpoint refusal republishes liveness`() {
        assertTrue(
            registerFailureNeedsLivenessRepublishUtil(
                "controller_error: replica_register_stale_editor_endpoint: identity does not match current reliable-liveness endpoint",
            ),
        )
        assertFalse(registerFailureNeedsLivenessRepublishUtil("controller_error: timeout"))
        assertFalse(registerFailureNeedsLivenessRepublishUtil(null))
    }

    @Test
    fun `an unreachable liveness report is retried with bounded backoff`() {
        assertEquals(250L, livenessReportRetryDelayMsUtil(LivenessReportOutcome.Retry, 0))
        assertEquals(500L, livenessReportRetryDelayMsUtil(LivenessReportOutcome.Retry, 1))
        assertEquals(4_000L, livenessReportRetryDelayMsUtil(LivenessReportOutcome.Retry, 6))
        assertNull(
            livenessReportRetryDelayMsUtil(LivenessReportOutcome.Retry, LIVENESS_REPORT_MAX_ATTEMPTS - 1),
        )
        assertNull(livenessReportRetryDelayMsUtil(LivenessReportOutcome.Published, 0))
        assertNull(livenessReportRetryDelayMsUtil(LivenessReportOutcome.NotSessionDocument, 0))
    }

    @Test
    fun `a retired generation never retries registration`() {
        val manager = source("CrdtReplicaManager.kt")
        val failure = manager.substringAfter("private fun recordRegisterFailure(")
            .substringBefore("private fun clearRegisterFailure(")
        val retiredReturn = failure.indexOf("if (PluginGeneration.retired) return")
        val schedule = failure.indexOf("scheduleRegisterRetry(")
        assertTrue(retiredReturn >= 0)
        assertTrue("the retired check must precede any retry", retiredReturn < schedule)
        val scheduler = manager.substringAfter("private fun scheduleRegisterRetry(")
            .substringBefore("private fun markLocalPending(")
        assertTrue(scheduler.contains("disposed.get() || PluginGeneration.retired) return"))
    }

    @Test
    fun `a failed open report is retried instead of dropped`() {
        val listener = source("ReliableSyncLivenessListener.kt")
        val report = listener.substringAfter("private fun reportOpenWithRetry(")
            .substringBefore("private fun reportOpenNow(")
        assertTrue(report.contains("livenessReportRetryDelayMsUtil(outcome, attempt)"))
        assertTrue(
            "a retry republishes, because the graph already marked the document open",
            report.contains("republish = attempt > 0"),
        )
    }
}
