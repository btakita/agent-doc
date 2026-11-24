package com.github.btakita.agentdoc.split.backend

import com.github.btakita.agentdoc.split.FrontendClientBuild
import com.github.btakita.agentdoc.split.FrontendReleaseGeneration
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * GH #435: [FrontendReleaseDeliveryGate] moves the multi-megabyte release to a Client only after
 * its schema answer, only to a Client on this backend's shell, and once per generation.
 */
class BackendSurfaceSnapshotRpcApiTest {
    private var now = 0L
    private val gate = FrontendReleaseDeliveryGate(backendShell = { "b".repeat(64) }, clock = { now })
    private val generation = FrontendReleaseGeneration(
        releaseVersion = "0.2.1053",
        packageUrl = "https://example.invalid/agent-doc-jetbrains-262-0.2.1053.zip",
        implSha256 = "e".repeat(64),
        shellDigest = "b".repeat(64),
        spiVersion = 1,
        epoch = 9,
    )

    @Test
    fun theBytesWaitForTheSchemaAnswerAndTheClientShell() {
        assertEquals(FrontendReleaseDeliveryGate.Admission.NOT_NEGOTIATED, gate.admission("c"))
        gate.negotiated("c")
        assertEquals(FrontendReleaseDeliveryGate.Admission.SHELL_UNKNOWN, gate.admission("c"))
        assertEquals(FrontendReleaseDeliveryGate.ShellObservation.FIRST_MATCH, gate.observeShell("c", "b".repeat(64)))
        assertEquals(FrontendReleaseDeliveryGate.ShellObservation.UNCHANGED, gate.observeShell("c", "b".repeat(64)))
        assertEquals(FrontendReleaseDeliveryGate.Admission.DELIVER, gate.admission("c"))
        gate.leaseOpened("c")
        assertEquals(
            FrontendReleaseDeliveryGate.Admission.NOT_NEGOTIATED,
            gate.admission("c"),
            "a new lease negotiates again before the bytes move",
        )
        assertEquals("b".repeat(64), gate.clientShell("c"), "the proven shell survives a new lease")
    }

    @Test
    fun anotherShellIsNeverAdmitted() {
        gate.negotiated("c")
        assertEquals(
            FrontendReleaseDeliveryGate.ShellObservation.FIRST_MISMATCH,
            gate.observeBuild("c", FrontendClientBuild("0.2.1046", "/p", "a".repeat(64), 1)),
        )
        assertEquals(FrontendReleaseDeliveryGate.Admission.SHELL_MISMATCH, gate.admission("c"))
        assertEquals("0.2.1046", gate.clientBuild("c")?.pluginVersion)
    }

    @Test
    fun oneGenerationIsDeliveredOncePerBackoffAndEachWithholdReasonLogsOnce() {
        assertTrue(gate.withheld("c", FrontendReleaseDeliveryGate.Admission.NOT_NEGOTIATED))
        assertFalse(gate.withheld("c", FrontendReleaseDeliveryGate.Admission.NOT_NEGOTIATED))
        assertTrue(gate.withheld("c", FrontendReleaseDeliveryGate.Admission.SHELL_UNKNOWN))
        assertTrue(gate.claimDelivery("c", generation))
        now += 30_000
        assertFalse(gate.claimDelivery("c", generation))
        assertTrue(gate.claimDelivery("c", generation.copy(epoch = 10)), "a new generation goes at once")
        now += FrontendReleaseDeliveryGate.RELEASE_REFETCH_AFTER_MS
        assertTrue(gate.claimDelivery("c", generation.copy(epoch = 10)))
        assertTrue(gate.claimDelivery("other", generation), "per Client")
    }

    @Test
    fun theGateKeepsOnlyTheNewestClients() {
        repeat(FrontendReleaseDeliveryGate.MAX_TRACKED_CLIENTS + 10) { index ->
            now++
            gate.observeShell("c$index", "a".repeat(64))
            gate.leaseOpened("c$index")
        }
        assertNull(gate.clientShell("c0"))
        assertEquals("a".repeat(64), gate.clientShell("c${FrontendReleaseDeliveryGate.MAX_TRACKED_CLIENTS + 9}"))
    }
}
