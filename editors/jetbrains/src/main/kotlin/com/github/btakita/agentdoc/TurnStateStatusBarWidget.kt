package com.github.btakita.agentdoc

import com.github.btakita.agentdoc.shell.TurnStateStatus
import com.intellij.openapi.Disposable
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.Disposer

/**
 * What the shell's turn-state status-bar widget (`ShellTurnStateStatusBarWidget`, GH #339 R1)
 * shows. It reads the project turn-state cache updated by the event-driven
 * [TurnStateBannerRefresher], so painting never calls native projection APIs on a Swing timer.
 */
internal object TurnStateStatusBarWidget {
    private const val BRAND = "agent-doc"
    private const val DEFAULT_TOOLTIP =
        "Agent Doc turn state — the Project Controller's authoritative turn phase for this document"

    fun watch(project: Project, parent: Disposable, onChange: Runnable) {
        val refresher = TurnStateBannerRefresher.getInstance(project)
        refresher.start()
        Disposer.register(parent, refresher.addListener(TurnStateBannerRefresher.Listener { onChange.run() }))
    }

    fun requestRefresh(project: Project, reason: String) {
        TurnStateBannerRefresher.getInstance(project).requestSelectedRefresh(reason)
    }

    fun status(project: Project): TurnStateStatus {
        val file = FileEditorManager.getInstance(project).selectedFiles
            .firstOrNull { it.name.endsWith(".md") }
        // Always show a visible, non-empty indicator so the widget stays findable
        // and never collapses to zero width: the Project Controller turn phase
        // when a turn is in flight, "$BRAND: idle" on a markdown document,
        // otherwise the bare brand.
        val presentation = file?.let {
            TurnStateBannerRefresher.getInstance(project).cachedPresentationFor(it.path)
        }
        val text = if (file == null) {
            BRAND
        } else {
            presentation
                ?.label
                ?.ifEmpty { "$BRAND: idle" }
                ?: "$BRAND: idle"
        }
        return TurnStateStatus(text, presentation?.tooltip ?: DEFAULT_TOOLTIP, file?.path)
    }
}
