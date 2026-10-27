package com.github.btakita.agentdoc

import com.github.btakita.agentdoc.shell.TurnStateBanner
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile

/**
 * The editor banner that surfaces the Project Controller's authoritative turn phase across the top
 * of an agent-doc markdown file (goal 1's visible surface). The shell's
 * `ShellTurnStateBannerProvider` (GH #339 R1) asks [banner] and builds the strip itself
 * (`ShellTurnStateStrips`).
 *
 * Reads the cached projection maintained by [TurnStateBannerRefresher]. The banner is shown only
 * while a turn is in flight (persisting / awaiting response); it is hidden when idle so it never
 * permanently consumes editor space. Native projection reads run only on the refresher event loop,
 * not from notification collection.
 */
internal object TurnStateBannerProvider {
    fun banner(project: Project, file: VirtualFile): TurnStateBanner? {
        if (!file.name.endsWith(".md")) return null
        if (PluginGeneration.retired) return null
        val refresher = TurnStateBannerRefresher.getInstance(project)
        refresher.start()
        // GH #323: this collection edge runs for every editor that shows the file, including a
        // Remote Dev client's, whose open/selection edges never reach the project
        // FileEditorManager topic. Admit the document here so its controller frames render.
        refresher.observeDisplayed(file, "editor-notification")
        // Empty label == idle / not-an-agent-doc-turn → no banner.
        val presentation = refresher.cachedPresentationFor(file.path)
        val label = presentation.label
        if (label.isEmpty() || !presentation.showBanner) return null
        return TurnStateBanner(label, presentation.tooltip)
    }
}
