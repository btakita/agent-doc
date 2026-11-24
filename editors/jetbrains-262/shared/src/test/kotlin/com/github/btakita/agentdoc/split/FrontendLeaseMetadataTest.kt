package com.github.btakita.agentdoc.split

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

/** GH #441: the plugin version a frontend names in its lease's `frontendInstanceId`. */
class FrontendLeaseMetadataTest {
    @Test
    fun theInstanceIdCarriesThePluginVersion() {
        val id = frontendLeaseInstanceId("3f0c1d2e-uuid", "0.2.1053")
        assertEquals("3f0c1d2e-uuid;plugin=0.2.1053", id)
        assertEquals("0.2.1053", frontendLeasePluginVersion(id))
    }

    @Test
    fun aFrontendBeforeTheMetadataNamesNoPlugin() {
        assertNull(frontendLeasePluginVersion("3f0c1d2e-uuid"), "0.2.1052 and earlier send a bare UUID")
        assertNull(frontendLeasePluginVersion("3f0c1d2e-uuid;plugin="))
    }

    @Test
    fun anUnknownVersionIsNotSent() {
        assertEquals("3f0c1d2e-uuid", frontendLeaseInstanceId("3f0c1d2e-uuid", null))
        assertEquals("3f0c1d2e-uuid", frontendLeaseInstanceId("3f0c1d2e-uuid", "unknown"))
        assertEquals("3f0c1d2e-uuid", frontendLeaseInstanceId("3f0c1d2e-uuid", " "))
    }
}
