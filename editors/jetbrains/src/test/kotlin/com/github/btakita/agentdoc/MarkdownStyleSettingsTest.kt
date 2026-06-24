package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Test
import java.awt.Font

class MarkdownStyleSettingsTest {

    @Test
    fun `StructureStyle defaults are fully inheriting`() {
        val s = MarkdownStyleSettings.StructureStyle()
        assertEquals(MarkdownStyleSettings.INHERIT, s.fgLightRgb)
        assertEquals(MarkdownStyleSettings.INHERIT, s.fgDarkRgb)
        assertEquals(MarkdownStyleSettings.INHERIT, s.bgLightRgb)
        assertEquals(MarkdownStyleSettings.INHERIT, s.bgDarkRgb)
        assertEquals(MarkdownStyleSettings.INHERIT, s.fontStyle)
    }

    @Test
    fun `getInstance falls back to inheriting defaults outside the platform`() {
        val settings = MarkdownStyleSettings.getInstance()
        val style = settings.styleFor("prompt")
        assertEquals(MarkdownStyleSettings.INHERIT, style.fgLightRgb)
        assertEquals(MarkdownStyleSettings.INHERIT, style.fontStyle)
    }

    @Test
    fun `loadState round-trips per-structure overrides`() {
        val settings = MarkdownStyleSettings()
        val styles = LinkedHashMap<String, MarkdownStyleSettings.StructureStyle>()
        styles["prompt"] = MarkdownStyleSettings.StructureStyle(
            fgLightRgb = 0xFF6B01, fgDarkRgb = 0x006CAC, fontStyle = Font.BOLD,
        )
        styles["boundary"] = MarkdownStyleSettings.StructureStyle(
            bgLightRgb = 0xECE9E9, fontStyle = Font.ITALIC,
        )
        settings.loadState(MarkdownStyleSettings.MarkdownStyleState(styles))

        val prompt = settings.styleFor("prompt")
        assertEquals(0xFF6B01, prompt.fgLightRgb)
        assertEquals(0x006CAC, prompt.fgDarkRgb)
        assertEquals(Font.BOLD, prompt.fontStyle)
        val boundary = settings.styleFor("boundary")
        assertEquals(0xECE9E9, boundary.bgLightRgb)
        assertEquals(Font.ITALIC, boundary.fontStyle)
    }

    @Test
    fun `missing structure returns inheriting default`() {
        val settings = MarkdownStyleSettings()
        val style = settings.styleFor("component_body")
        assertEquals(MarkdownStyleSettings.INHERIT, style.bgDarkRgb)
    }

    @Test
    fun `STRUCTURE_KINDS covers every attrsFor branch`() {
        val expected = listOf(
            "component_body", "component_open", "component_close",
            "patch_open", "patch_close", "boundary",
            "scratch_comment", "scratch_comment_body",
            "bold", "italic", "prompt", "response_heading",
            "tracked_id", "label_tag",
        )
        assertEquals(expected, MarkdownStyleSettings.STRUCTURE_KINDS)
    }

    @Test
    fun `fontStyleFor inherits fallback when unset`() {
        MarkdownStyleSettings.getInstance().also {
            it.loadState(MarkdownStyleSettings.MarkdownStyleState(LinkedHashMap()))
        }
        assertEquals(Font.BOLD, MarkdownStyleSettings.fontStyleFor("prompt", Font.BOLD))
    }

    @Test
    fun `fontStyleFor returns override when set`() {
        val settings = MarkdownStyleSettings.getInstance()
        val styles = LinkedHashMap<String, MarkdownStyleSettings.StructureStyle>()
        styles["prompt"] = MarkdownStyleSettings.StructureStyle(fontStyle = Font.ITALIC)
        settings.loadState(MarkdownStyleSettings.MarkdownStyleState(styles))
        assertEquals(Font.ITALIC, MarkdownStyleSettings.fontStyleFor("prompt", Font.BOLD))
        assertNotNull(settings.rawStyles()["prompt"])
    }
}
