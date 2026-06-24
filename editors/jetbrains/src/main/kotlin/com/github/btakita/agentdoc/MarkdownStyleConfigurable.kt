package com.github.btakita.agentdoc

import com.intellij.openapi.options.SearchableConfigurable
import com.intellij.ui.JBColor
import com.intellij.util.ui.FormBuilder
import java.awt.Color
import java.awt.Dimension
import java.awt.Graphics
import java.awt.event.MouseEvent
import javax.swing.JButton
import javax.swing.JCheckBox
import javax.swing.JColorChooser
import javax.swing.JComboBox
import javax.swing.JComponent
import javax.swing.JPanel
import javax.swing.border.LineBorder

/**
 * Settings page: `Settings → Tools → Agent Doc Markdown Style`.
 *
 * Exposes per-structure foreground (light/dark), background (light/dark), and
 * font style overrides for every agent-doc Markdown structure highlighted by
 * [VisualHighlighterManager]. Every field defaults to "Inherit" so an
 * untouched page is a pure pass-through to the editor color scheme.
 *
 * Backed by [MarkdownStyleSettings].
 */
class MarkdownStyleConfigurable : SearchableConfigurable {

    private val settings = MarkdownStyleSettings.getInstance()

    private enum class FontOption(val label: String, val value: Int) {
        INHERIT("Inherit", MarkdownStyleSettings.INHERIT),
        PLAIN("Plain", java.awt.Font.PLAIN),
        BOLD("Bold", java.awt.Font.BOLD),
        ITALIC("Italic", java.awt.Font.ITALIC),
        BOLD_ITALIC("Bold + Italic", java.awt.Font.BOLD or java.awt.Font.ITALIC);

        override fun toString(): String = label
    }

    private data class Row(
        val kind: String,
        val fgLight: InheritColorButton,
        val fgDark: InheritColorButton,
        val bgLight: InheritColorButton,
        val bgDark: InheritColorButton,
        val fontCombo: JComboBox<FontOption>,
        val reset: JButton,
    )

    private val rows = mutableListOf<Row>()

    override fun getId(): String = "com.github.btakita.agentdoc.markdownStyleConfigurable"

    override fun getDisplayName(): String = "Agent Doc Markdown Style"

    override fun createComponent(): JComponent {
        val builder = FormBuilder.createFormBuilder()
        for (kind in MarkdownStyleSettings.STRUCTURE_KINDS) {
            val style = settings.styleFor(kind)
            val fgLight = InheritColorButton(style.fgLightRgb)
            val fgDark = InheritColorButton(style.fgDarkRgb)
            val bgLight = InheritColorButton(style.bgLightRgb)
            val bgDark = InheritColorButton(style.bgDarkRgb)
            val fontCombo = JComboBox(FontOption.entries.toTypedArray())
            fontCombo.selectedItem = FontOption.entries.firstOrNull { it.value == style.fontStyle }
                ?: FontOption.INHERIT
            val reset = JButton("Reset").apply {
                toolTipText = "Reset '$kind' to inherit all defaults"
                addActionListener {
                    fgLight.reset()
                    fgDark.reset()
                    bgLight.reset()
                    bgDark.reset()
                    fontCombo.selectedItem = FontOption.INHERIT
                }
            }
            val row = Row(kind, fgLight, fgDark, bgLight, bgDark, fontCombo, reset)
            rows += row

            val panel = JPanel()
            panel.add(fgLight)
            panel.add(fgDark)
            panel.add(bgLight)
            panel.add(bgDark)
            panel.add(fontCombo)
            panel.add(reset)
            builder.addLabeledComponent(
                "<html><b>$kind</b>" +
                    "  <font color='gray' size='2'>fg·L / fg·D / bg·L / bg·D / font</font></html>",
                panel,
            )
        }
        return builder.panel
    }

    override fun isModified(): Boolean = rows.any { it.isModified() }

    override fun apply() {
        val styles = LinkedHashMap<String, MarkdownStyleSettings.StructureStyle>()
        for (row in rows) {
            styles[row.kind] = MarkdownStyleSettings.StructureStyle(
                fgLightRgb = row.fgLight.value,
                fgDarkRgb = row.fgDark.value,
                bgLightRgb = row.bgLight.value,
                bgDarkRgb = row.bgDark.value,
                fontStyle = (row.fontCombo.selectedItem as FontOption).value,
            )
        }
        settings.loadState(MarkdownStyleSettings.MarkdownStyleState(styles))
    }

    override fun reset() {
        for (row in rows) {
            val style = settings.styleFor(row.kind)
            row.fgLight.setValue(style.fgLightRgb)
            row.fgDark.setValue(style.fgDarkRgb)
            row.bgLight.setValue(style.bgLightRgb)
            row.bgDark.setValue(style.bgDarkRgb)
            row.fontCombo.selectedItem = FontOption.entries.firstOrNull { it.value == style.fontStyle }
                ?: FontOption.INHERIT
        }
    }

    override fun disposeUIResources() {
        rows.clear()
    }

    private fun Row.isModified(): Boolean {
        val style = settings.styleFor(kind)
        return fgLight.value != style.fgLightRgb ||
            fgDark.value != style.fgDarkRgb ||
            bgLight.value != style.bgLightRgb ||
            bgDark.value != style.bgDarkRgb ||
            (fontCombo.selectedItem as FontOption).value != style.fontStyle
    }

    /**
     * Color swatch button that supports an "inherit" sentinel ([MarkdownStyleSettings.INHERIT]).
     * When inheriting it renders a gray dashed swatch; clicking opens the Swing
     * color chooser to pick an explicit override.
     */
    private class InheritColorButton(initial: Int) : JButton() {
        var value: Int = initial
            private set

        init {
            preferredSize = Dimension(56, 24)
            isFocusable = false
            isContentAreaFilled = false
            isBorderPainted = false
            isOpaque = false
            border = LineBorder(JBColor.border(), 1)
            addActionListener {
                val seed = if (value == MarkdownStyleSettings.INHERIT) Color.LIGHT_GRAY else Color(value and 0xFFFFFF, false)
                val chosen = JColorChooser.showDialog(this, "Select color (Cancel = inherit)", seed) ?: return@addActionListener
                value = chosen.rgb and 0xFFFFFF
                repaint()
            }
        }

        fun setValue(rgb: Int) {
            value = rgb
            repaint()
        }

        fun reset() {
            value = MarkdownStyleSettings.INHERIT
            repaint()
        }

        override fun paintComponent(g: Graphics) {
            if (value == MarkdownStyleSettings.INHERIT) {
                g.color = JBColor(Color(220, 220, 220), Color(70, 70, 70))
                g.fillRect(0, 0, width, height)
                g.color = JBColor(Color(150, 150, 150), Color(120, 120, 120))
                val mid = height / 2
                g.drawLine(0, mid, width, mid)
            } else {
                g.color = Color(value and 0xFFFFFF, false)
                g.fillRect(0, 0, width, height)
            }
        }

        override fun getToolTipText(e: MouseEvent?): String =
            if (value == MarkdownStyleSettings.INHERIT) "Inherit (click to override)"
            else String.format("#%06X", value and 0xFFFFFF)
    }
}
