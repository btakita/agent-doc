package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.LinkedHashMap

class MarketplacePlatformBridgeTest {
    @Test
    fun `pane enumeration prefers the list getter over the map getter`() {
        val methods =
            MapPaneSource::class.java.methods + ListPaneSource::class.java.methods
        val selected = MarketplacePlatformBridge.selectToolWindowPanesMethod(methods)

        assertEquals(List::class.java, selected?.returnType)
        assertEquals(
            listOf("pane-list"),
            MarketplacePlatformBridge.toolWindowPaneInstances(selected?.invoke(ListPaneSource())),
        )
    }

    @Test
    fun `a platform that only publishes the map still yields pane values`() {
        val selected =
            MarketplacePlatformBridge.selectToolWindowPanesMethod(MapPaneSource::class.java.methods)
        assertEquals(LinkedHashMap::class.java, selected?.returnType)
        assertEquals(
            listOf("pane-map"),
            MarketplacePlatformBridge.toolWindowPaneInstances(selected?.invoke(MapPaneSource())),
        )
    }

    @Test
    fun `pane frame lookup keeps a window getter and ignores unrelated zero-arg methods`() {
        val selected =
            MarketplacePlatformBridge.selectPaneFrameMethod(FrameSource::class.java.methods)
        assertTrue(selected?.name?.startsWith("getFrame") == true)
        assertSame(java.awt.Window::class.java, selected?.returnType)
    }
}

class MapPaneSource {
    fun getToolWindowPanes(): LinkedHashMap<String, String> = linkedMapOf("id" to "pane-map")
}

class ListPaneSource {
    fun getToolWindowPanes(): List<String> = listOf("pane-list")
}

class FrameSource {
    fun getFractOffsetInRootPane(): Int = 0

    fun getFrame(): java.awt.Window? = null
}
