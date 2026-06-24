package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Test

class AgentDocColorSettingsTest {

    @Test
    fun `ColorState defaults match the briantakita_me sitewide palette`() {
        val s = AgentDocColorSettings.ColorState()
        // fill (page background)
        assertEquals(0xFCFEFB, s.fillLightRgb)
        assertEquals(0x212737, s.fillDarkRgb)
        // text-base
        assertEquals(0x282728, s.textBaseLightRgb)
        assertEquals(0xEAEDF3, s.textBaseDarkRgb)
        // card (secondary surface) — dark raised by #lzdarktheme for contrast
        assertEquals(0xE6E6E6, s.cardLightRgb)
        assertEquals(0x5C6E9E, s.cardDarkRgb)
        // border
        assertEquals(0xECE9E9, s.borderLightRgb)
        assertEquals(0xAB4B08, s.borderDarkRgb)
        // accent
        assertEquals(0x006CAC, s.accentLightRgb)
        assertEquals(0xFF6B01, s.accentDarkRgb)
    }

    @Test
    fun `getInstance falls back to defaults outside the platform`() {
        // Plain unit test (no application) → fallback instance with default state.
        val state = AgentDocColorSettings.getInstance().state
        assertEquals(0x5C6E9E, state.cardDarkRgb)
        assertEquals(0xFF6B01, state.accentDarkRgb)
    }

    @Test
    fun `loadState round-trips overridden values while keeping defaults for the rest`() {
        val settings = AgentDocColorSettings()
        settings.loadState(AgentDocColorSettings.ColorState().apply { accentDarkRgb = 0x123456 })
        assertEquals(0x123456, settings.state.accentDarkRgb)
        assertEquals(0xFCFEFB, settings.state.fillLightRgb)
        assertEquals(0x5C6E9E, settings.state.cardDarkRgb)
    }
}
