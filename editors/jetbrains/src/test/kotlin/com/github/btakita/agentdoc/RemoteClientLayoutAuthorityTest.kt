package com.github.btakita.agentdoc

import com.intellij.openapi.project.Project
import java.lang.reflect.Proxy
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * GH #320 option B: while the modular backend forwards the JetBrains Client's measured columns,
 * the Remote Dev membership fold must not publish the layout; with no forward it stays the
 * fallback. Ownership is per forwarded endpoint, so one retiring connection never releases
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
