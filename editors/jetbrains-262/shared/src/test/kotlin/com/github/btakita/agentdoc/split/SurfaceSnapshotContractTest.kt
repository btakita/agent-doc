package com.github.btakita.agentdoc.split

import kotlin.test.Test
import kotlin.test.assertEquals

class SurfaceSnapshotContractTest {
    @Test
    fun rootPaneIsTheOnlyMainRoleToken() {
        assertEquals("root", MAIN_SURFACE_PANE_ID)
    }
}
