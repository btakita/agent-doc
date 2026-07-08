package com.github.btakita.agentdoc

import com.intellij.openapi.fileEditor.FileEditor
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.ui.EditorNotificationProvider
import com.intellij.ui.JBColor
import com.intellij.ui.components.JBLabel
import com.intellij.util.ui.JBUI
import java.awt.BorderLayout
import java.awt.Dimension
import java.util.function.Function
import javax.swing.JComponent
import javax.swing.JPanel

/**
 * Editor banner that surfaces the CPC's authoritative turn phase across the top of
 * an agent-doc markdown file (goal 1's visible surface). Unlike the status-bar
 * widget — which the IntelliJ 2026.1 platform instantiates but silently never
 * paints — an [EditorNotificationProvider] renders a real editor component and
 * throws loudly if it fails, so it is both reliable and diagnosable.
 *
 * Reads the cached projection maintained by [TurnStateBannerRefresher]. The
 * banner is shown only while a turn is in flight (persisting / awaiting
 * response); it is hidden when idle so it never permanently consumes editor
 * space. Native projection reads run only on the refresher event loop, not from
 * notification collection.
 */
class TurnStateBannerProvider : EditorNotificationProvider {
    override fun collectNotificationData(
        project: Project,
        file: VirtualFile,
    ): Function<in FileEditor, out JComponent?>? {
        if (!file.name.endsWith(".md")) return null
        val refresher = TurnStateBannerRefresher.getInstance(project)
        refresher.start()
        refresher.requestRefresh(file, "banner-collect")
        // Empty label == idle / not-an-agent-doc-turn → no banner.
        val presentation = refresher.cachedPresentationFor(file.path)
        val label = presentation.label
        if (label.isEmpty() || !presentation.showBanner) return null
        return Function { _ ->
            // A very thin single-line strip instead of the full-height
            // EditorNotificationPanel, so it barely consumes document space.
            JPanel(BorderLayout()).apply {
                isOpaque = true
                background = STRIP_BG
                border = JBUI.Borders.empty(0, 8)
                add(
                    JBLabel(label).apply {
                        font = JBUI.Fonts.smallFont()
                        foreground = STRIP_FG
                    },
                    BorderLayout.WEST,
                )
                val h = JBUI.scale(STRIP_HEIGHT_DP)
                minimumSize = Dimension(0, h)
                preferredSize = Dimension(0, h)
                maximumSize = Dimension(Int.MAX_VALUE, h)
            }
        }
    }

    private companion object {
        private const val STRIP_HEIGHT_DP = 18
        // Subtle info-tone strip that reads on both light and dark themes.
        private val STRIP_BG = JBColor(0xEAF1FB, 0x2A3A4A)
        private val STRIP_FG = JBColor(0x3B5273, 0xA9C7EA)
    }
}
