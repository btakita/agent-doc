package com.github.btakita.agentdoc

import com.intellij.openapi.Disposable
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.wm.StatusBar
import com.intellij.openapi.wm.StatusBarWidget
import com.intellij.openapi.wm.StatusBarWidgetFactory

/**
 * Status-bar widget that makes the CPC turn-state coordination VISIBLE in the
 * JetBrains editor (goal 1). Reads the project turn-state cache updated by the
 * event-driven [TurnStateBannerRefresher], so painting never calls native
 * projection APIs on a Swing timer. Parity with the VS Code status-bar indicator
 * (`specs/14-realtime-workflow.md` § Editor Parity Requirement).
 */
class TurnStateStatusBarWidgetFactory : StatusBarWidgetFactory {
    override fun getId(): String = TurnStateStatusBarWidget.WIDGET_ID
    override fun getDisplayName(): String = "Agent Doc Turn State"
    override fun isAvailable(project: Project): Boolean = true
    override fun createWidget(project: Project): StatusBarWidget = TurnStateStatusBarWidget(project)
    override fun disposeWidget(widget: StatusBarWidget) = widget.dispose()
    override fun canBeEnabledOn(statusBar: StatusBar): Boolean = true
}

class TurnStateStatusBarWidget(private val project: Project) :
    StatusBarWidget, StatusBarWidget.TextPresentation {

    companion object {
        const val WIDGET_ID = "com.github.btakita.agentdoc.TurnStateStatusBar"
        private const val BRAND = "agent-doc"
        private const val DEFAULT_TOOLTIP =
            "Agent Doc turn state — the CPC's authoritative turn phase for this document"
        private val LOG = Logger.getInstance(TurnStateStatusBarWidget::class.java)
    }

    private var statusBar: StatusBar? = null
    private var listenerDisposable: Disposable? = null
    // Never empty: an empty TextPresentation makes the platform build a zero-width
    // TextPanel at creation that a later updateWidget won't re-grow, so the widget
    // stays invisible. Seed with the brand so the component has a paintable size
    // from the first render.
    private var widgetText: String = BRAND
    private var widgetTooltip: String = DEFAULT_TOOLTIP

    override fun ID(): String = WIDGET_ID

    override fun getPresentation(): StatusBarWidget.WidgetPresentation = this

    override fun install(statusBar: StatusBar) {
        this.statusBar = statusBar
        val refresher = TurnStateBannerRefresher.getInstance(project)
        refresher.start()
        listenerDisposable = refresher.addListener(TurnStateBannerRefresher.Listener {
            refreshFromCache()
        })
        LOG.info("[turn-widget] installed on status bar; using event-driven turn-state cache")
        refreshFromCache()
        refresher.requestSelectedRefresh("statusbar-install")
    }

    override fun dispose() {
        listenerDisposable?.dispose()
        listenerDisposable = null
        statusBar = null
    }

    private fun refreshFromCache() {
        val file = FileEditorManager.getInstance(project).selectedFiles
            .firstOrNull { it.name.endsWith(".md") }
        // Always show a visible, non-empty indicator so the widget stays findable
        // and never collapses to zero width: the CPC turn phase when a turn is in
        // flight, "$BRAND: idle" on a markdown document, otherwise the bare brand.
        val presentation = file?.let {
            TurnStateBannerRefresher.getInstance(project).cachedPresentationFor(it.path)
        }
        val next = if (file == null) {
            BRAND
        } else {
            presentation
                ?.label
                ?.ifEmpty { "$BRAND: idle" }
                ?: "$BRAND: idle"
        }
        val nextTooltip = presentation?.tooltip ?: DEFAULT_TOOLTIP
        if (next != widgetText || nextTooltip != widgetTooltip) {
            LOG.info("[turn-widget] refresh: file=${file?.path ?: "(none)"} text=\"$next\"")
            widgetText = next
            widgetTooltip = nextTooltip
            statusBar?.updateWidget(WIDGET_ID)
        }
    }

    override fun getText(): String = widgetText
    override fun getAlignment(): Float = 0f
    override fun getTooltipText(): String = widgetTooltip
}
