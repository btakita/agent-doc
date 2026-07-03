package com.github.btakita.agentdoc

import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.wm.StatusBar
import com.intellij.openapi.wm.StatusBarWidget
import com.intellij.openapi.wm.StatusBarWidgetFactory
import com.intellij.util.Alarm

/**
 * Status-bar widget that makes the CPC turn-state coordination VISIBLE in the
 * JetBrains editor (goal 1). Polls [TurnStateBridge] — which calls the
 * `agent_doc_turn_projection` FFI — for the active markdown document and shows the
 * CPC's turn phase ("⟳ agent-doc: awaiting response" / "persisting"), or nothing
 * when idle. Parity with the VS Code status-bar indicator
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
        private const val REFRESH_MS = 1500
        private val LOG = Logger.getInstance(TurnStateStatusBarWidget::class.java)
    }

    private var statusBar: StatusBar? = null
    private val alarm = Alarm(Alarm.ThreadToUse.SWING_THREAD, this)
    private var widgetText: String = ""

    override fun ID(): String = WIDGET_ID

    override fun getPresentation(): StatusBarWidget.WidgetPresentation = this

    override fun install(statusBar: StatusBar) {
        this.statusBar = statusBar
        LOG.info("[turn-widget] installed on status bar; polling every ${REFRESH_MS}ms")
        scheduleRefresh()
    }

    override fun dispose() {
        alarm.cancelAllRequests()
        statusBar = null
    }

    private fun scheduleRefresh() {
        alarm.addRequest({
            refresh()
            if (statusBar != null) scheduleRefresh()
        }, REFRESH_MS)
    }

    private fun refresh() {
        val file = FileEditorManager.getInstance(project).selectedFiles
            .firstOrNull { it.name.endsWith(".md") }
        // Always show a visible indicator on a markdown document so the widget is
        // findable: the CPC turn phase when a turn is in flight, otherwise "idle".
        // Hidden (empty) only for non-markdown files.
        val next = if (file == null) {
            ""
        } else {
            TurnStateBridge.presentationForFile(file.path).label.ifEmpty { "agent-doc: idle" }
        }
        if (next != widgetText) {
            LOG.info("[turn-widget] refresh: file=${file?.path ?: "(none)"} text=\"$next\"")
            widgetText = next
            statusBar?.updateWidget(WIDGET_ID)
        }
    }

    override fun getText(): String = widgetText
    override fun getAlignment(): Float = 0f
    override fun getTooltipText(): String =
        "Agent Doc turn state — the CPC's authoritative turn phase for this document"
}
