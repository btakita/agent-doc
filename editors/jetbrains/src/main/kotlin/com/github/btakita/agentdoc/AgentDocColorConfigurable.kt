package com.github.btakita.agentdoc

import com.intellij.openapi.options.SearchableConfigurable
import com.intellij.ui.JBColor
import com.intellij.util.ui.FormBuilder
import java.awt.Color
import java.awt.Dimension
import java.awt.Graphics
import java.awt.event.MouseEvent
import javax.swing.JButton
import javax.swing.JColorChooser
import javax.swing.JComponent
import javax.swing.border.LineBorder

/**
 * Settings page: `Settings → Tools → Agent Doc Colors`.
 *
 * Exposes the five Agent Doc semantic colors (`fill`, `text-base`, `card`,
 * `border`, `accent`) for both light and dark themes, defaulting to the
 * briantakita.me sitewide palette. Backed by [AgentDocColorSettings].
 */
class AgentDocColorConfigurable : SearchableConfigurable {

    private val settings = AgentDocColorSettings.getInstance()

    private data class Slot(
        val label: String,
        val group: String,
        val read: () -> Int,
        val write: (Int) -> Unit,
    )

    private val slots: List<Slot> = run {
        val s = settings.state
        listOf(
            Slot("Fill · light", "Background (--color-fill)",
                { s.fillLightRgb }, { s.fillLightRgb = it }),
            Slot("Fill · dark", "Background (--color-fill)",
                { s.fillDarkRgb }, { s.fillDarkRgb = it }),
            Slot("Text base · light", "Foreground (--color-text-base)",
                { s.textBaseLightRgb }, { s.textBaseLightRgb = it }),
            Slot("Text base · dark", "Foreground (--color-text-base)",
                { s.textBaseDarkRgb }, { s.textBaseDarkRgb = it }),
            Slot("Card · light", "Surface (--color-card)",
                { s.cardLightRgb }, { s.cardLightRgb = it }),
            Slot("Card · dark", "Surface (--color-card)",
                { s.cardDarkRgb }, { s.cardDarkRgb = it }),
            Slot("Border · light", "Dividers (--color-border)",
                { s.borderLightRgb }, { s.borderLightRgb = it }),
            Slot("Border · dark", "Dividers (--color-border)",
                { s.borderDarkRgb }, { s.borderDarkRgb = it }),
            Slot("Accent · light", "Focus / actions (--color-accent)",
                { s.accentLightRgb }, { s.accentLightRgb = it }),
            Slot("Accent · dark", "Focus / actions (--color-accent)",
                { s.accentDarkRgb }, { s.accentDarkRgb = it }),
        )
    }

    private val buttons = LinkedHashMap<String, ColorButton>()

    override fun getId(): String = "com.github.btakita.agentdoc.colorConfigurable"

    override fun getDisplayName(): String = "Agent Doc Colors"

    override fun createComponent(): JComponent {
        val builder = FormBuilder.createFormBuilder()
        for (slot in slots) {
            val button = ColorButton(Color(slot.read() and 0xFFFFFF, false))
            buttons[slot.label] = button
            builder.addLabeledComponent("${slot.label}  (${slot.group})", button)
        }
        return builder.panel
    }

    override fun isModified(): Boolean =
        slots.any { (buttons[it.label]?.color?.rgb ?: 0) and 0xFFFFFF != it.read() and 0xFFFFFF }

    override fun apply() {
        for (slot in slots) {
            buttons[slot.label]?.let { slot.write(it.color.rgb and 0xFFFFFF) }
        }
    }

    override fun reset() {
        for (slot in slots) {
            buttons[slot.label]?.color = Color(slot.read() and 0xFFFFFF, false)
        }
    }

    override fun disposeUIResources() {
        buttons.clear()
    }

    /** Color swatch button that opens the Swing color chooser on click. */
    private class ColorButton(color: Color) : JButton() {
        var color: Color = color
            set(value) {
                field = value
                repaint()
            }

        init {
            preferredSize = Dimension(80, 24)
            isFocusable = false
            isContentAreaFilled = false
            isBorderPainted = false
            isOpaque = false
            border = LineBorder(JBColor.border(), 1)
            toolTipText = "Click to choose"
            addActionListener {
                val chosen = JColorChooser.showDialog(this, "Select color", this.color) ?: return@addActionListener
                this.color = chosen
            }
        }

        override fun paintComponent(g: Graphics) {
            g.color = color
            g.fillRect(0, 0, width, height)
        }

        override fun getToolTipText(e: MouseEvent?): String =
            String.format("#%06X", color.rgb and 0xFFFFFF)
    }
}
