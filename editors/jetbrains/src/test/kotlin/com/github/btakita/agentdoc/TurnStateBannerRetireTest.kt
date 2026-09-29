package com.github.btakita.agentdoc

import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.file.Files
import java.nio.file.Paths
import javax.swing.JLabel
import javax.swing.JPanel

/**
 * `#staleturnbanner`: a dynamic plugin upgrade left the old generation's "awaiting response"
 * strip attached to `lazily.md` (two stacked strips, then one permanent strip), because the
 * platform only recomputes panels for providers that are still registered.
 */
class TurnStateBannerRetireTest {
    @After
    fun tearDown() {
        TurnStateBannerProvider.retireStrips()
    }

    @Test
    fun `retiring detaches every strip the generation attached`() {
        val editorTop = JPanel()
        val stale = JPanel().apply { add(JLabel("⟳ agent-doc: awaiting response")) }
        val sibling = JPanel()
        editorTop.add(stale)
        editorTop.add(sibling)
        TurnStateBannerProvider.trackStripForTest(stale)

        assertEquals(1, TurnStateBannerProvider.retireStrips())

        assertNull("the frozen strip must leave the editor", stale.parent)
        assertFalse(stale.isVisible)
        assertEquals(listOf(sibling), editorTop.components.toList())
        assertEquals(0, TurnStateBannerProvider.trackedStripCountForTest())
        assertEquals("retiring twice removes nothing more", 0, TurnStateBannerProvider.retireStrips())
    }

    @Test
    fun `plugin unload retires strips after the generation is marked retired`() {
        val source = Files.readString(
            listOf(
                Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PluginLifecycleListener.kt"),
                Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PluginLifecycleListener.kt"),
            ).first { Files.exists(it) },
        )
        val cleanup = source.substring(source.indexOf("class PluginUnloadCleanupService"))
        val retireGeneration = cleanup.indexOf("PluginGeneration.retire()")
        val retireStrips = cleanup.indexOf("TurnStateBannerProvider.retireStrips()")

        assertTrue(retireGeneration >= 0)
        assertTrue(
            "strips are removed only once the provider can no longer create new ones",
            retireStrips > retireGeneration,
        )
    }
}
