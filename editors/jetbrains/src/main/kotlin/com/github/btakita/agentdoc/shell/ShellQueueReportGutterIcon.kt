package com.github.btakita.agentdoc.shell

import com.intellij.openapi.editor.markup.GutterIconRenderer
import java.awt.Component
import java.awt.Graphics
import java.awt.Graphics2D
import java.awt.RenderingHints
import javax.swing.Icon
import javax.swing.UIManager

/** Stable shell-owned renderer: range highlighters may outlive an implementation generation. */
class ShellQueueReportGutterIconRenderer(
    private val glyph: String,
    private val tooltip: String,
) : GutterIconRenderer() {
    private val textIcon = QueueReportTextIcon(glyph)

    override fun getIcon(): Icon = textIcon
    override fun getTooltipText(): String = tooltip
    override fun getAlignment(): Alignment = Alignment.RIGHT

    override fun equals(other: Any?): Boolean =
        other is ShellQueueReportGutterIconRenderer && glyph == other.glyph && tooltip == other.tooltip

    override fun hashCode(): Int = 31 * glyph.hashCode() + tooltip.hashCode()
}

private class QueueReportTextIcon(private val text: String) : Icon {
    override fun getIconWidth(): Int = if (text.length > 1) 24 else 16
    override fun getIconHeight(): Int = 16

    override fun paintIcon(component: Component?, graphics: Graphics, x: Int, y: Int) {
        val g = graphics.create() as Graphics2D
        try {
            g.setRenderingHint(RenderingHints.KEY_TEXT_ANTIALIASING, RenderingHints.VALUE_TEXT_ANTIALIAS_ON)
            g.font = (UIManager.getFont("Label.font") ?: g.font).deriveFont(12f)
            g.color = UIManager.getColor("Label.foreground") ?: g.color
            val metrics = g.fontMetrics
            val drawX = x + ((iconWidth - metrics.stringWidth(text)) / 2).coerceAtLeast(0)
            val drawY = y + (iconHeight - metrics.height) / 2 + metrics.ascent
            g.drawString(text, drawX, drawY)
        } finally {
            g.dispose()
        }
    }
}
