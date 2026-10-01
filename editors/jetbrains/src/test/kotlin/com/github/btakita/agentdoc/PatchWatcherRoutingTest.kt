package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * #jbmultiprojectroute: one native socket callback serves every project that shares a
 * root, so each message must reach the project that owns its file.
 */
class PatchWatcherRoutingTest {
    private val projects = listOf("/w/agent-loop", "/w/agent-loop/src/haiven-dev", "/w/other")

    @Test
    fun `the deepest project containing the file owns it`() {
        assertEquals(
            "/w/agent-loop/src/haiven-dev",
            owningBasePathUtil(projects, "/w/agent-loop/src/haiven-dev/tasks/sdk.md"),
        )
        assertEquals(
            "/w/agent-loop",
            owningBasePathUtil(projects, "/w/agent-loop/tasks/agent-doc/agent-doc-bugs.md"),
        )
    }

    @Test
    fun `a sibling prefix is not ownership`() {
        assertNull(owningBasePathUtil(listOf("/w/agent-loop"), "/w/agent-loop-2/x.md"))
    }

    @Test
    fun `a file outside every open project falls back to the receiving watcher`() {
        assertNull(owningBasePathUtil(projects, "/elsewhere/doc.md"))
    }
}
