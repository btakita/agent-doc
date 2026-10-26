package com.github.btakita.agentdoc.split.backend

import com.github.btakita.agentdoc.AgentDocPopupAction
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Test
import org.w3c.dom.Element
import java.io.File
import javax.xml.parsers.DocumentBuilderFactory

/**
 * GH #375: under Remote Dev the backend builds `AgentDoc.Popup` from its own ActionManager, which
 * holds only the actions in `agent.doc.backend.xml`. Every popup id must be registered there
 * unless the popup treats it as client-only, and every client-only id must belong to the
 * frontend descriptor, so the backend popup never meets an unexpected null.
 */
class BackendPopupActionRegistrationTest {
    private fun declaredActionIds(xml: java.io.InputStream): Set<String> {
        val document = DocumentBuilderFactory.newInstance().newDocumentBuilder().parse(xml)
        val nodes = document.getElementsByTagName("action")
        return (0 until nodes.length).map { (nodes.item(it) as Element).getAttribute("id") }.toSet()
    }

    private val backendIds: Set<String> by lazy {
        val descriptor = javaClass.classLoader.getResourceAsStream("agent.doc.backend.xml")
        assertNotNull("generated agent.doc.backend.xml must be on the test classpath", descriptor)
        descriptor!!.use(::declaredActionIds)
    }

    private val frontendIds: Set<String> by lazy {
        File("../frontend/src/main/resources/agent.doc.frontend.xml").inputStream().use(::declaredActionIds)
    }

    private val popupIds = AgentDocPopupAction.PRIMARY_ACTION_IDS + AgentDocPopupAction.OVERFLOW_ACTION_IDS

    @Test
    fun `every backend popup id is registered in the backend or declared client-only`() {
        val unresolvable = popupIds.filter { it !in backendIds && it !in AgentDocPopupAction.CLIENT_ONLY_ACTION_IDS }
        assertEquals("popup ids the Remote Dev backend cannot resolve", emptyList<String>(), unresolvable)
    }

    @Test
    fun `client-only popup ids are frontend-owned and absent from the backend`() {
        val clientOnly = AgentDocPopupAction.CLIENT_ONLY_ACTION_IDS
        assertEquals("client-only ids registered in the backend", emptySet<String>(), clientOnly intersect backendIds)
        assertEquals("client-only ids the frontend does not register", emptySet<String>(), clientOnly - frontendIds)
    }

    @Test
    fun `backend popup resolves without adding null`() {
        val unexpected = mutableListOf<String>()
        val resolved = AgentDocPopupAction.resolveActions(
            popupIds,
            { id -> if (id in backendIds) object : com.intellij.openapi.actionSystem.AnAction() {
                override fun actionPerformed(e: com.intellij.openapi.actionSystem.AnActionEvent) = Unit
            } else null },
        ) { unexpected += it }
        assertEquals(emptyList<String>(), unexpected)
        assertEquals(popupIds.count { it in backendIds }, resolved.size)
    }
}
