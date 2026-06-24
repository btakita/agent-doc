package com.github.btakita.agentdoc

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.components.PersistentStateComponent
import com.intellij.openapi.components.State
import com.intellij.openapi.components.Storage
import com.intellij.ui.JBColor
import java.awt.Color
import java.awt.Font

/**
 * User-configurable, per-structure Markdown style overrides for the in-editor
 * agent-doc highlighting emitted by [VisualHighlighterManager.attrsFor].
 *
 * Each agent-doc structure (`component_body`, `boundary`, `prompt`, etc.) can
 * override the foreground, background, and font style it would otherwise derive
 * from the IntelliJ editor color scheme (`DefaultLanguageHighlighterColors.*`).
 * A sentinel value of `-1` ([INHERIT]) means "use the color-scheme-derived
 * default" so users only override the fields they care about.
 *
 * Settings UI: `Settings → Tools → Agent Doc Markdown Style`. Changes take
 * effect on the next highlighter refresh (doc edit / save).
 */
@State(name = "MarkdownStyleSettings", storages = [Storage("agent-doc-markdown-style.xml")])
class MarkdownStyleSettings :
    PersistentStateComponent<MarkdownStyleSettings.MarkdownStyleState> {

    /**
     * Persisted override table. Keys are agent-doc structure kinds (the same
     * `kind` strings produced by `NativePatching.visualTokens` and consumed by
     * [VisualHighlighterManager.attrsFor]). Missing entries inherit defaults.
     */
    data class MarkdownStyleState(
        var styles: MutableMap<String, StructureStyle> = LinkedHashMap(),
    )

    /**
     * Per-structure style override. Every field defaults to [INHERIT] (`-1`)
     * so an untouched entry is a pure pass-through to the color-scheme default.
     *
     * - `fgLightRgb` / `fgDarkRgb`: foreground RGB (`0xRRGGBB`), or [INHERIT].
     * - `bgLightRgb` / `bgDarkRgb`: background RGB (`0xRRGGBB`), or [INHERIT].
     * - `fontStyle`: one of [Font].[PLAIN], [Font].[BOLD], [Font].[ITALIC],
     *   `Font.BOLD | Font.ITALIC`, or [INHERIT] (`-1`) to keep the default.
     */
    data class StructureStyle(
        var fgLightRgb: Int = INHERIT,
        var fgDarkRgb: Int = INHERIT,
        var bgLightRgb: Int = INHERIT,
        var bgDarkRgb: Int = INHERIT,
        var fontStyle: Int = INHERIT,
    )

    @Volatile
    private var myState = MarkdownStyleState()

    override fun getState(): MarkdownStyleState = myState

    override fun loadState(loaded: MarkdownStyleState) {
        myState = loaded
    }

    /** Returns the override for [kind], or a fully-inheriting default if absent. */
    fun styleFor(kind: String): StructureStyle =
        myState.styles[kind] ?: StructureStyle()

    /** Visible for the settings page / tests. */
    fun rawStyles(): MutableMap<String, StructureStyle> = myState.styles

    companion object {
        /** Sentinel: inherit the color-scheme-derived default instead of overriding. */
        const val INHERIT: Int = -1

        /** All agent-doc structure kinds exposed by [VisualHighlighterManager]. */
        val STRUCTURE_KINDS: List<String> = listOf(
            "component_body",
            "component_open",
            "component_close",
            "patch_open",
            "patch_close",
            "boundary",
            "scratch_comment",
            "scratch_comment_body",
            "bold",
            "italic",
            "prompt",
            "response_heading",
            "tracked_id",
            "label_tag",
        )

        private val FALLBACK = MarkdownStyleSettings()

        @JvmStatic
        fun getInstance(): MarkdownStyleSettings =
            runCatching {
                ApplicationManager.getApplication()?.getService(MarkdownStyleSettings::class.java)
            }.getOrNull() ?: FALLBACK

        /**
         * Resolves the foreground [JBColor] for [kind]: the user override when
         * set, otherwise [fallback].
         */
        fun foregroundFor(kind: String, fallback: JBColor): JBColor {
            val s = getInstance().styleFor(kind)
            return if (s.fgLightRgb == INHERIT && s.fgDarkRgb == INHERIT) {
                fallback
            } else {
                JBColor(
                    Color(rgbOrFallback(s.fgLightRgb, fallback), false),
                    Color(rgbOrFallback(s.fgDarkRgb, fallback), false),
                )
            }
        }

        /**
         * Resolves the background [Color] for [kind]: a [JBColor] built from the
         * user's light/dark override when set, otherwise [fallback]. The
         * [JBColor] auto-switches with the IDE LaF.
         */
        fun backgroundFor(kind: String, fallback: Color): Color {
            val s = getInstance().styleFor(kind)
            return if (s.bgLightRgb == INHERIT && s.bgDarkRgb == INHERIT) {
                fallback
            } else {
                JBColor(
                    Color(rgbOrFallback(s.bgLightRgb, toJB(fallback)), false),
                    Color(rgbOrFallback(s.bgDarkRgb, toJB(fallback)), false),
                )
            }
        }

        /**
         * Resolves the font style for [kind]: the user override when set
         * (`Font.BOLD`, `Font.ITALIC`, …), otherwise [fallback].
         */
        fun fontStyleFor(kind: String, fallback: Int): Int {
            val s = getInstance().styleFor(kind)
            return if (s.fontStyle == INHERIT) fallback else s.fontStyle
        }

        private fun rgbOrFallback(rgb: Int, fallback: JBColor): Int =
            if (rgb == INHERIT) fallback.rgb and 0xFFFFFF else rgb and 0xFFFFFF

        /** Wraps a plain [Color] as a [JBColor] so [rgbOrFallback] can read it. */
        private fun toJB(color: Color): JBColor =
            if (color is JBColor) color else JBColor(color, color)
    }
}
