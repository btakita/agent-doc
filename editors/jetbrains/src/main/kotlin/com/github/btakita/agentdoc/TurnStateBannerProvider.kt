package com.github.btakita.agentdoc

import com.intellij.openapi.fileEditor.FileEditor
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.ui.EditorNotificationPanel
import com.intellij.ui.EditorNotificationProvider
import java.util.function.Function
import javax.swing.JComponent

/**
 * Editor banner that surfaces the CPC's authoritative turn phase across the top of
 * an agent-doc markdown file (goal 1's visible surface). Unlike the status-bar
 * widget — which the IntelliJ 2026.1 platform instantiates but silently never
 * paints — an [EditorNotificationProvider] renders a real editor component and
 * throws loudly if it fails, so it is both reliable and diagnosable.
 *
 * Reads the same `agent_doc_turn_projection` FFI via [TurnStateBridge], so it
 * inherits the proven CPC↔plugin coordination. The banner is shown only while a
 * turn is in flight (persisting / awaiting response); it is hidden when idle so it
 * never permanently consumes editor space. [TurnStateBannerRefresher] drives
 * re-collection on phase transitions.
 */
class TurnStateBannerProvider : EditorNotificationProvider {
    override fun collectNotificationData(
        project: Project,
        file: VirtualFile,
    ): Function<in FileEditor, out JComponent?>? {
        if (!file.name.endsWith(".md")) return null
        // Empty label == idle / not-an-agent-doc-turn → no banner.
        val label = TurnStateBridge.presentationForFile(file.path).label
        if (label.isEmpty()) return null
        return Function { _ ->
            EditorNotificationPanel().apply {
                text(label)
            }
        }
    }
}
