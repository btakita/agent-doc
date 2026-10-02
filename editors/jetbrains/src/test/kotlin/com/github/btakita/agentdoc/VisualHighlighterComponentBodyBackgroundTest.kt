package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.awt.Color

/**
 * Regression: plugin 0.2.344 set the default `component_body` background to the
 * raw editor background, so the element-content highlighter painted nothing
 * visible ("The element content should have a background. It's gone now.").
 */
class VisualHighlighterComponentBodyBackgroundTest {

    private fun distance(a: Color, b: Color): Int =
        maxOf(
            kotlin.math.abs(a.red - b.red),
            kotlin.math.abs(a.green - b.green),
            kotlin.math.abs(a.blue - b.blue),
        )

    @Test
    fun `component body background is visibly distinct from a light editor background`() {
        val background = Color(0xFF, 0xFF, 0xFF)
        val foreground = Color(0x08, 0x08, 0x08)
        val body = VisualHighlighterManager.componentBodyBackground(background, foreground)

        assertNotEquals(background, body)
        assertTrue("tint too faint: $body", distance(background, body) >= 10)
        assertTrue("component body should darken a light theme", body.red < background.red)
    }

    @Test
    fun `component body background is visibly distinct from a dark editor background`() {
        val background = Color(0x2B, 0x2B, 0x2B)
        val foreground = Color(0xBB, 0xBB, 0xBB)
        val body = VisualHighlighterManager.componentBodyBackground(background, foreground)

        assertNotEquals(background, body)
        assertTrue("tint too faint: $body", distance(background, body) >= 10)
        assertTrue("component body should lighten a dark theme", body.red > background.red)
    }

    @Test
    fun `component body background stays neutral for a neutral scheme`() {
        val body = VisualHighlighterManager.componentBodyBackground(
            Color(0x1E, 0x1F, 0x22),
            Color(0xBC, 0xBE, 0xC4),
        )
        // Tinting toward the theme foreground, not a syntax accent (the retired
        // METADATA-green wash), keeps the channels close together.
        assertTrue("component body should be neutral: $body", distance(Color(body.red, body.red, body.red), body) <= 4)
    }

    @Test
    fun `component body background stays a subtle wash`() {
        val background = Color(0xFF, 0xFF, 0xFF)
        val body = VisualHighlighterManager.componentBodyBackground(background, Color.BLACK)
        assertTrue("tint too strong for text legibility: $body", distance(background, body) <= 32)
        assertEquals(body.red, body.green)
        assertEquals(body.green, body.blue)
    }
}
