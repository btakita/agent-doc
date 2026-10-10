package com.github.btakita.agentdoc.split

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class SurfaceSnapshotContractTest {
    @Test
    fun rootPaneIsTheOnlyMainRoleToken() {
        assertEquals("root", MAIN_SURFACE_PANE_ID)
    }

    @Test
    fun terminalCapabilityAdmissionIsExplicitAndExhaustive() {
        assertEquals(
            listOf(
                FrontendPresentationCapability.SNAPSHOT_ONLY,
                FrontendPresentationCapability.EXACT_262_INTERNAL,
            ),
            FrontendPresentationCapability.entries,
        )
        assertFalse(FrontendPresentationCapability.SNAPSHOT_ONLY.admitsTerminalPresentation())
        assertTrue(FrontendPresentationCapability.EXACT_262_INTERNAL.admitsTerminalPresentation())
    }
}
