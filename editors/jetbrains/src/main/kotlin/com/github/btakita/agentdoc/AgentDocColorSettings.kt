package com.github.btakita.agentdoc

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.components.PersistentStateComponent
import com.intellij.openapi.components.State
import com.intellij.openapi.components.Storage
import com.intellij.ui.JBColor
import java.awt.Color

/**
 * User-configurable Agent Doc prompt-panel colors for light and dark themes.
 *
 * Defaults mirror the briantakita.me sitewide CSS variables
 * (`config/blog_site.ts` `color_scheme_vars`) so the plugin and the site share
 * one palette: `--color-fill`, `--color-text-base`, `--color-card`,
 * `--color-border`, `--color-accent`. Each semantic color has a light and a
 * dark RGB value, exposed as a `JBColor` that auto-switches with the IDE LaF.
 *
 * Settings UI: `Settings → Tools → Agent Doc Colors`. Changes take effect on
 * the next prompt-panel show.
 */
@State(name = "AgentDocColorSettings", storages = [Storage("agent-doc-colors.xml")])
class AgentDocColorSettings : PersistentStateComponent<AgentDocColorSettings.ColorState> {

    /**
     * Persisted color values. RGB stored as `0xRRGGBB` ints. Defaults are the
     * briantakita.me palette; users override via the settings page.
     */
    data class ColorState(
        var fillLightRgb: Int = 0xFCFEFB,
        var fillDarkRgb: Int = 0x212737,
        var textBaseLightRgb: Int = 0x282728,
        var textBaseDarkRgb: Int = 0xEAEDF3,
        var cardLightRgb: Int = 0xE6E6E6,
        var cardDarkRgb: Int = 0x5C6E9E,
        var borderLightRgb: Int = 0xECE9E9,
        var borderDarkRgb: Int = 0xAB4B08,
        var accentLightRgb: Int = 0x006CAC,
        var accentDarkRgb: Int = 0xFF6B01,
    )

    @Volatile
    private var state = ColorState()

    override fun getState(): ColorState = state

    override fun loadState(loaded: ColorState) {
        state = loaded
    }

    companion object {
        /** Fallback used when the application/service is unavailable (e.g. plain unit tests). */
        private val FALLBACK = AgentDocColorSettings()

        @JvmStatic
        fun getInstance(): AgentDocColorSettings =
            runCatching {
                ApplicationManager.getApplication()?.getService(AgentDocColorSettings::class.java)
            }.getOrNull() ?: FALLBACK

        /** `--color-fill` — page/editor background surface. */
        fun fill(): JBColor {
            val s = getInstance().state
            return JBColor(Color(rgb(s.fillLightRgb), false), Color(rgb(s.fillDarkRgb), false))
        }

        /** `--color-text-base` — label / editor foreground. */
        fun textBase(): JBColor {
            val s = getInstance().state
            return JBColor(Color(rgb(s.textBaseLightRgb), false), Color(rgb(s.textBaseDarkRgb), false))
        }

        /** `--color-card` — secondary surface (prompt-panel background). */
        fun card(): JBColor {
            val s = getInstance().state
            return JBColor(Color(rgb(s.cardLightRgb), false), Color(rgb(s.cardDarkRgb), false))
        }

        /** `--color-border` — dividers / unfocused borders. */
        fun border(): JBColor {
            val s = getInstance().state
            return JBColor(Color(rgb(s.borderLightRgb), false), Color(rgb(s.borderDarkRgb), false))
        }

        /** `--color-accent` — focused border / actions. */
        fun accent(): JBColor {
            val s = getInstance().state
            return JBColor(Color(rgb(s.accentLightRgb), false), Color(rgb(s.accentDarkRgb), false))
        }

        private fun rgb(value: Int): Int = value and 0xFFFFFF
    }
}
