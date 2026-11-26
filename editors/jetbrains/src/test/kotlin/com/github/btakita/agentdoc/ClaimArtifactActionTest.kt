package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

class ClaimArtifactActionTest {
    @Test
    fun `default control path sits beside the artifact and keeps the stem`() {
        assertEquals("plans/my-plan.session.md", ClaimArtifactAction.defaultControlSessionPath("plans/my-plan.md"))
        assertEquals("README.session.md", ClaimArtifactAction.defaultControlSessionPath("./README.md"))
        assertEquals("notes.session.md", ClaimArtifactAction.defaultControlSessionPath("notes.MD"))
    }

    @Test
    fun `control path must be a different markdown file`() {
        assertTrue(ClaimArtifactAction.acceptableControlPath("plan.session.md", "plan.md"))
        assertFalse(ClaimArtifactAction.acceptableControlPath("plan.md", "./plan.md"))
        assertFalse(ClaimArtifactAction.acceptableControlPath("notes.txt", "plan.md"))
        assertFalse(ClaimArtifactAction.acceptableControlPath("/tmp/plan.session.md", "plan.md"))
    }

    @Test
    fun `init command names the control session and the artifact`() {
        assertEquals(
            listOf("agent-doc", "init", "plan.session.md", "--artifact", "plan.md"),
            ClaimArtifactAction.buildInitCommand("agent-doc", "plan.session.md", "plan.md"),
        )
    }

    @Test
    fun `plugin command ids stay stable and titles name control and artifact sessions`() {
        val xml = File("src/main/resources/META-INF/plugin.xml").readText()
        assertTrue(xml.contains("id=\"AgentDoc.Claim\""))
        assertTrue(xml.contains("text=\"Claim Control Session\""))
        assertTrue(xml.contains("text=\"Force Claim Control Session\""))
        assertTrue(xml.contains("id=\"AgentDoc.ClaimArtifact\""))
        assertTrue(xml.contains("text=\"Claim Artifact Session\""))
        assertFalse(xml.contains("Claim Session File"))
        assertFalse(xml.contains("agent:artifact"))
    }
}
