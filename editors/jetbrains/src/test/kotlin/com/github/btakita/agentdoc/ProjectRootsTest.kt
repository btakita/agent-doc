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

    @Test
    fun `stray state only dir in a skill folder resolves to the enclosing project`() {
        val project = fixture.newFolder("project")
        check(File(project, ".agent-doc").mkdirs())
        File(project, ".agent-doc/config.toml").writeText("")
        val runbooks = File(project, ".claude/skills/agent-doc/runbooks")
        check(File(runbooks, ".agent-doc/logs").mkdirs())
        val memory = File(project, "memory")
        check(File(memory, ".agent-doc/logs").mkdirs())
        val nested = File(project, "src/nested")
        check(File(nested, ".agent-doc").mkdirs())
        File(nested, ".agent-doc/config.toml").writeText("")
        val policy = ProjectRoots.Policy(null, emptyList(), listOf(fixture.root), false)

        assertEquals(project, policy.find(runbooks))
        assertFalse(policy.allows(runbooks))
        assertEquals(project, policy.find(memory))
        assertEquals(nested, policy.find(nested))

        val bare = fixture.newFolder("bare")
        check(File(bare, ".agent-doc").mkdirs())
        assertEquals(bare, policy.find(bare))
    }
}
