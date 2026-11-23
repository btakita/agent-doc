package com.github.btakita.agentdoc

import com.intellij.openapi.project.Project
import java.lang.reflect.Proxy
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * GH #320 option B: while the modular backend forwards the JetBrains Client's measured columns,
 * flat Remote Dev backend evidence must not publish structural layout; with no forward it stays
 * unknown. Ownership is per forwarded endpoint, so one retiring connection never releases
 * another's authority.
 */
class RemoteClientLayoutAuthorityTest {
    @Test
    fun measuredForwardOwnsTheLayoutUntilEveryEndpointReleases() {
        val project = fakeProject()
        val other = fakeProject()
        assertFalse(RemoteClientLayoutAuthority.isActive(project))

        RemoteClientLayoutAuthority.activate(project, "remote-client:a:1")
        RemoteClientLayoutAuthority.activate(project, "remote-client:b:2")
        assertTrue(RemoteClientLayoutAuthority.isActive(project))
        assertFalse(RemoteClientLayoutAuthority.isActive(other))

        RemoteClientLayoutAuthority.release(project, "remote-client:a:1")
        assertTrue(RemoteClientLayoutAuthority.isActive(project))
        RemoteClientLayoutAuthority.release(project, "remote-client:b:2")
        assertFalse(RemoteClientLayoutAuthority.isActive(project))
        RemoteClientLayoutAuthority.release(project, "remote-client:b:2")
        assertFalse(RemoteClientLayoutAuthority.isActive(project))
    }

    /**
     * GH #435: a route that defers to the measured layout names each owner's publication state,
     * so `frontend_measured` with no geometry says which hop has not delivered it.
     */
    @Test
    fun describeNamesEachOwnersPublicationStateUntilItReleases() {
        val project = fakeProject()
        assertNull(RemoteClientLayoutAuthority.describe(project))

        RemoteClientLayoutAuthority.activate(project, "remote-client:b:2")
        RemoteClientLayoutAuthority.activate(project, "remote-client:a:1")
        assertEquals(
            "frontend_layout=[remote-client:a:1:awaiting_first_snapshot,remote-client:b:2:awaiting_first_snapshot]",
            RemoteClientLayoutAuthority.describe(project),
        )

        RemoteClientLayoutAuthority.note(project, "remote-client:a:1", "snapshot_refused(DELIVERY_FAILED)")
        RemoteClientLayoutAuthority.note(project, "remote-client:unknown:9", "forwarded")
        assertEquals(
            "frontend_layout=[remote-client:a:1:snapshot_refused(DELIVERY_FAILED)," +
                "remote-client:b:2:awaiting_first_snapshot]",
            RemoteClientLayoutAuthority.describe(project),
        )

        RemoteClientLayoutAuthority.release(project, "remote-client:a:1")
        RemoteClientLayoutAuthority.note(project, "remote-client:a:1", "forwarded")
        assertEquals(
            "frontend_layout=[remote-client:b:2:awaiting_first_snapshot]",
            RemoteClientLayoutAuthority.describe(project),
        )
        RemoteClientLayoutAuthority.release(project, "remote-client:b:2")
        assertNull(RemoteClientLayoutAuthority.describe(project))
    }

    private fun fakeProject(): Project =
        Proxy.newProxyInstance(Project::class.java.classLoader, arrayOf(Project::class.java)) { proxy, method, args ->
            when (method.name) {
                "equals" -> proxy === args?.firstOrNull()
                "hashCode" -> System.identityHashCode(proxy)
                "toString" -> "fake-project"
                else -> throw UnsupportedOperationException(method.name)
            }
        } as Project
}
