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

/**
 * `#duplicateturnbanner`: lazily.md showed two stacked "awaiting response" strips after the
 * 2026-09-29 17:38 dynamic reload. The live generation removes any other turn strip beside its own.
 */
class TurnStateBannerSweepTest {
    @After
    fun tearDown() {
        PluginGeneration.resetForTest()
        TurnStateBannerProvider.retireStrips()
    }

    @Test
    fun `a new strip removes a previous generation's strip beside it`() {
        val editorTop = JPanel()
        val legacy = JPanel().apply { add(JLabel("⟳ agent-doc: awaiting response")) }
        val marked = JPanel().apply {
            putClientProperty(TurnStateBannerProvider.STRIP_MARKER_KEY, "another-generation")
        }
        val unrelated = JPanel().apply { add(JLabel("Markdown preview is disabled")) }
        val live = TurnStateBannerProvider.createStrip("⟳ agent-doc: awaiting response", null)!!
        listOf(legacy, marked, unrelated, live).forEach(editorTop::add)

        assertEquals(2, TurnStateBannerProvider.sweepForeignTurnStrips(editorTop, live))

        assertEquals(listOf(unrelated, live), editorTop.components.toList())
        assertNull(legacy.parent)
        assertNull(marked.parent)
    }

    @Test
    fun `wrapped top components are swept one level down, never the live strip`() {
        val editorTop = JPanel()
        val staleWrapper = JPanel().apply { add(JPanel().apply { add(JLabel("⚠ agent-doc: input required")) }) }
        val live = TurnStateBannerProvider.createStrip("⟳ agent-doc: persisting", null)!!
        val liveWrapper = JPanel().apply { add(live) }
        editorTop.add(staleWrapper)
        editorTop.add(liveWrapper)

        assertEquals(1, TurnStateBannerProvider.sweepForeignTurnStrips(editorTop, live))

        assertEquals(0, staleWrapper.componentCount)
        assertEquals(listOf(live), liveWrapper.components.toList())
    }

    @Test
    fun `a retired generation builds no strip`() {
        PluginGeneration.retire()
        assertNull(TurnStateBannerProvider.createStrip("⟳ agent-doc: awaiting response", null))
        assertEquals(0, TurnStateBannerProvider.trackedStripCountForTest())
    }
}
