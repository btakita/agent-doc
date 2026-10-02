package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.project.DumbAware
import java.io.File
import javax.xml.parsers.DocumentBuilderFactory
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.w3c.dom.Element

/**
 * The agent-doc menu must stay usable while the IDE is indexing ("Analyzing
 * project..."). During dumb mode the platform disables every action, and skips
 * every completion contributor and editor-notification provider, that is not
 * [DumbAware]. None of agent-doc's surfaces read PSI indexes, so all of them must
 * be dumb-aware; a new non-DumbAware action fails here.
 */
class DumbModeAvailabilityTest {
    private val pluginXml = File("src/main/resources/META-INF/plugin.xml")

    private fun pluginClassNames(tag: String, attribute: String): List<String> {
        val document = DocumentBuilderFactory.newInstance().newDocumentBuilder().parse(pluginXml)
        val nodes = document.getElementsByTagName(tag)
        return (0 until nodes.length)
            .map { (nodes.item(it) as Element).getAttribute(attribute) }
            .filter { it.isNotBlank() }
    }

    private fun load(name: String): Class<*> =
        Class.forName(name, false, DumbModeAvailabilityTest::class.java.classLoader)

    private fun notDumbAware(names: List<String>): List<String> =
        names.filterNot { DumbAware::class.java.isAssignableFrom(load(it)) }

    @Test
    fun `every registered agent-doc action is DumbAware`() {
        val actions = pluginClassNames("action", "class")
        assertTrue("plugin.xml should register agent-doc actions", actions.size >= 20)
        assertEquals("actions disabled while the IDE indexes", emptyList<String>(), notDumbAware(actions))
    }

    @Test
    fun `every registered agent-doc group is DumbAware`() {
        val groups = pluginClassNames("group", "class")
        assertEquals("groups hidden while the IDE indexes", emptyList<String>(), notDumbAware(groups))
    }

    @Test
    fun `popup groups built at runtime are DumbAware`() {
        assertTrue(DumbAware::class.java.isAssignableFrom(DumbAwareGroup::class.java))
        val source = File("src/main/kotlin/com/github/btakita/agentdoc/AgentDocPopupAction.kt").readText()
        assertFalse(
            "the Agent Doc popup must not build a plain DefaultActionGroup",
            Regex("""DefaultActionGroup\(\)\.apply|DefaultActionGroup\("More""").containsMatchIn(source),
        )
    }

    @Test
    fun `slash completion and turn banner stay active while indexing`() {
        val completion = pluginClassNames("completion.contributor", "implementationClass")
        val banners = pluginClassNames("editorNotificationProvider", "implementation")
        assertTrue(completion.isNotEmpty() && banners.isNotEmpty())
        assertEquals(emptyList<String>(), notDumbAware(completion + banners))
    }

    @Test
    fun `every AnAction subclass in the plugin sources is DumbAware`() {
        val sourceRoot = File("src/main/kotlin/com/github/btakita/agentdoc")
        val declaration = Regex("""(?m)^(?:internal |private )?class (\w+)\b[^{\n]*:\s*AnAction\(\)""")
        val actionClasses = sourceRoot.walkTopDown()
            .filter { it.isFile && it.extension == "kt" }
            .flatMap { file -> declaration.findAll(file.readText()).map { it.groupValues[1] } }
            .map { "com.github.btakita.agentdoc.$it" }
            .filter { AnAction::class.java.isAssignableFrom(load(it)) }
            .toList()
        assertTrue("expected agent-doc action classes in sources", actionClasses.size >= 20)
        assertEquals(emptyList<String>(), notDumbAware(actionClasses))
    }
}
