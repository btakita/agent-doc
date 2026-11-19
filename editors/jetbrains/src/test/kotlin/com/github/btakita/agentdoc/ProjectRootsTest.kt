package com.github.btakita.agentdoc

import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

class ProjectRootsTest {
    @get:Rule val fixture = TemporaryFolder()

    @Test
    fun `project discovery respects home temp and configured ceiling boundaries`() {
        val home = fixture.newFolder("home")
        val temp = fixture.newFolder("temp")
        val notes = File(home, "notes")
        val child = File(notes, "work")
        for (dir in listOf(home, temp, notes)) check(File(dir, ".agent-doc").mkdirs())
        check(child.mkdir())
        val policy = ProjectRoots.Policy(home, listOf(temp), listOf(notes), false)

        assertNull(policy.find(home))
        assertNull(policy.find(temp))
        assertEquals(notes, policy.find(notes))
        assertNull(policy.find(child))
        assertFalse(policy.allowsFallback(child, home))
        assertFalse(policy.allowsFallback(child, notes))
        assertTrue(policy.allowsFallback(notes, notes))
        assertFalse(policy.allows(home.toPath().root.toFile()))

        assertEquals(home, ProjectRoots.Policy(home, listOf(temp), emptyList(), true).find(home))
        check(File(home, ".git").mkdir())
        assertEquals(home, policy.find(home))
    }
}
