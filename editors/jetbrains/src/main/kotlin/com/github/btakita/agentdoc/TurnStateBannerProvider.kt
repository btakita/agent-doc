package com.github.btakita.agentdoc

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.fileEditor.FileEditor
import com.intellij.openapi.project.DumbAware
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.ui.EditorNotificationProvider
import com.intellij.ui.JBColor
import com.intellij.ui.components.JBLabel
import com.intellij.util.ui.JBUI
import java.awt.BorderLayout
import java.awt.Dimension
import java.util.Collections
import java.util.WeakHashMap
import java.util.function.Function
import javax.swing.JComponent
import javax.swing.JPanel

/**
 * Editor banner that surfaces the Project Controller's authoritative turn phase across the top of
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
class TurnStateBannerProvider : EditorNotificationProvider, DumbAware {
    override fun collectNotificationData(
        project: Project,
        file: VirtualFile,
    ): Function<in FileEditor, out JComponent?>? {
        if (!file.name.endsWith(".md")) return null
        if (PluginGeneration.retired) return null
        val refresher = TurnStateBannerRefresher.getInstance(project)
        refresher.start()
        // Empty label == idle / not-an-agent-doc-turn → no banner.
        val presentation = refresher.cachedPresentationFor(file.path)
        val label = presentation.label
        if (label.isEmpty() || !presentation.showBanner) return null
        // The platform may apply this factory on the EDT after the generation retired
        // (`#duplicateturnbanner`), so the factory re-checks instead of trusting the check above.
        return Function { _ -> createStrip(label, presentation.tooltip) }
    }

    /**
     * `#duplicateturnbanner`: a turn strip that removes every other agent-doc turn strip beside
     * it when it attaches. Only one turn phase exists per document, so any sibling strip is one
     * that an unloaded plugin generation left behind. Its own unload cleanup can miss it: the
     * platform can apply a provider factory on the EDT after that generation retired, and a
     * generation built before `#staleturnbanner` has no cleanup at all. lazily.md showed two
     * stacked "awaiting response" strips after the 2026-09-29 17:38 dynamic reload. The live
     * generation is the only one that can see and remove such a strip.
     */
    private class TurnStateStrip : JPanel(BorderLayout()) {
        init {
            putClientProperty(STRIP_MARKER_KEY, GENERATION_TOKEN)
        }

        override fun addNotify() {
            super.addNotify()
            val parent = parent ?: return
            if (PluginGeneration.retired) {
                detach(listOf(this))
            } else {
                // The platform may wrap each top component, so the siblings can sit one level up.
                sweepForeignTurnStrips(parent, this)
                parent.parent?.let { sweepForeignTurnStrips(it, this) }
            }
        }
    }

    internal companion object {
        private const val STRIP_HEIGHT_DP = 18
        // Subtle info-tone strip that reads on both light and dark themes.
        private val STRIP_BG = JBColor(0xEAF1FB, 0x2A3A4A)
        private val STRIP_FG = JBColor(0x3B5273, 0xA9C7EA)

        /** A string key, so a strip from another plugin classloader is still recognisable. */
        internal const val STRIP_MARKER_KEY = "agent-doc.turn-state-strip"

        /** Identifies this classloader's strips; each generation gets its own. */
        private val GENERATION_TOKEN: String = java.util.UUID.randomUUID().toString()

        private val TURN_LABEL_PREFIXES = listOf("⟳ agent-doc:", "⚠ agent-doc:", "agent-doc:")

        /** Build a strip, or nothing once this generation has retired. */
        internal fun createStrip(label: String, tooltip: String?): JComponent? {
            if (PluginGeneration.retired) return null
            // A very thin single-line strip instead of the full-height
            // EditorNotificationPanel, so it barely consumes document space.
            return TurnStateStrip().apply {
                isOpaque = true
                background = STRIP_BG
                border = JBUI.Borders.empty(0, 8)
                add(
                    JBLabel(label).apply {
                        font = JBUI.Fonts.smallFont()
                        foreground = STRIP_FG
                        toolTipText = tooltip
                    },
                    BorderLayout.WEST,
                )
                val h = JBUI.scale(STRIP_HEIGHT_DP)
                minimumSize = Dimension(0, h)
                preferredSize = Dimension(0, h)
                maximumSize = Dimension(Int.MAX_VALUE, h)
            }.also(::trackStrip)
        }

        /**
         * Whether [component] is an agent-doc turn strip: either marked (any generation since
         * `#duplicateturnbanner`) or, for older generations, a panel whose label is a turn phase.
         */
        internal fun isTurnStrip(component: java.awt.Component): Boolean {
            if (component !is JComponent) return false
            if (component.getClientProperty(STRIP_MARKER_KEY) != null) return true
            if (component !is JPanel) return false
            return component.components.any { child ->
                child is javax.swing.JLabel &&
                    TURN_LABEL_PREFIXES.any { prefix -> child.text?.startsWith(prefix) == true }
            }
        }

        /**
         * Remove every turn strip in [container] other than [keep], looking one wrapper level
         * down because the platform may wrap each top component. Returns the number removed.
         */
        internal fun sweepForeignTurnStrips(container: java.awt.Container, keep: JComponent): Int {
            val stale = container.components.flatMap { child ->
                when {
                    child === keep -> emptyList()
                    isTurnStrip(child) -> listOf(child as JComponent)
                    child is java.awt.Container && child.components.none { it === keep } ->
                        child.components.filter { it !== keep && isTurnStrip(it) }.map { it as JComponent }
                    else -> emptyList()
                }
            }
            if (stale.isNotEmpty()) detach(stale)
            return stale.size
        }

        private fun detach(components: List<JComponent>) {
            val run = {
                components.forEach { strip ->
                    strips.remove(strip)
                    strip.isVisible = false
                    strip.parent?.let { parent ->
                        parent.remove(strip)
                        parent.revalidate()
                        parent.repaint()
                    }
                }
            }
            val application = ApplicationManager.getApplication()
            if (application == null || application.isDispatchThread) run() else application.invokeLater(run)
        }

        /**
         * `#staleturnbanner`: every strip this classloader attached to an editor. A dynamic
         * plugin upgrade unregisters this provider but leaves the strips it already added in
         * place, and nothing refreshes them again: the old generation's refresher is disposed,
         * and the platform only recomputes panels for providers that are still registered. A
         * document that was mid-turn during the upgrade therefore kept a frozen
         * "awaiting response" strip beside the replacement generation's live one: two stacked
         * strips, then one permanent strip once the live one went idle. Weak keys, so a closed
         * editor's strip is not retained here.
         */
        private val strips: MutableSet<JComponent> =
            Collections.synchronizedSet(Collections.newSetFromMap(WeakHashMap()))

        private fun trackStrip(strip: JComponent) {
            strips.add(strip)
        }

        /**
         * Detach every strip this generation attached. Runs at the plugin-unload boundary, so
         * the replacement generation is the only one painting turn state. Returns the number
         * of strips removed.
         */
        fun retireStrips(): Int {
            val retired = synchronized(strips) { strips.toList().also { strips.clear() } }
            val detach = {
                retired.forEach { strip ->
                    strip.isVisible = false
                    strip.parent?.let { parent ->
                        parent.remove(strip)
                        parent.revalidate()
                        parent.repaint()
                    }
                }
            }
            val application = ApplicationManager.getApplication()
            if (application == null || application.isDispatchThread) detach() else application.invokeLater(detach)
            return retired.size
        }

        internal fun trackedStripCountForTest(): Int = strips.size

        internal fun trackStripForTest(strip: JComponent) = trackStrip(strip)
    }
}
