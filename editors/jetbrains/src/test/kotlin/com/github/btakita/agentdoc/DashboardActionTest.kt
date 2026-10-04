package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.w3c.dom.Element
import java.io.File
import javax.xml.parsers.DocumentBuilderFactory

class DashboardActionTest {
    @Test
    fun `dashboard writes the controller-owned default projection`() {
        assertEquals(listOf("dashboard", "--write"), DashboardAction.DASHBOARD_COMMAND_ARGS)
        assertEquals(
            File("/w/agent-loop/.agent-doc/dashboard.md").path,
            DashboardAction.dashboardPath("/w/agent-loop"),
        )
    }

    @Test
    fun `dashboard is project scoped, not markdown gated`() {
        assertTrue(DashboardAction.shouldEnable(hasProject = true))
        assertFalse(DashboardAction.shouldEnable(hasProject = false))
    }

    @Test
    fun `generated projection is never classified as a session document`() {
        val projection = """
            <!-- agent-doc-dashboard v1 scope=fleet all=false -->
            # Agent Doc dashboard

            | State | Document |
            |---|---|
            | STALLED | [tasks/agent\_doc\_notes.md](../tasks/agent%5Fdoc%5Fnotes.md) |
        """.trimIndent()
        assertFalse(isAgentDocDocumentTextUtil(projection))
    }

    @Test
    fun `dashboard action is declared in the menus and the popup`() {
        val document = DocumentBuilderFactory.newInstance().newDocumentBuilder()
            .parse(File("src/main/resources/META-INF/plugin.xml"))
        val nodes = document.getElementsByTagName("action")
        val action = (0 until nodes.length).map { nodes.item(it) as Element }
            .single { it.getAttribute("id") == "AgentDoc.Dashboard" }
        assertEquals("com.github.btakita.agentdoc.DashboardAction", action.getAttribute("class"))
        val groupNodes = action.getElementsByTagName("add-to-group")
        val groups = (0 until groupNodes.length).map { (groupNodes.item(it) as Element).getAttribute("group-id") }
        assertTrue("dashboard must be in the Tools menu: $groups", "ToolsMenu" in groups)
        assertTrue("dashboard must be in the editor context menu: $groups", "EditorPopupMenu" in groups)
        assertTrue(
            "dashboard must be reachable from the Agent Doc popup",
            "AgentDoc.Dashboard" in AgentDocPopupAction.PRIMARY_ACTION_IDS,
        )
    }
}
