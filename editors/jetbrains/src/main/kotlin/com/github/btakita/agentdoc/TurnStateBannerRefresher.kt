package com.github.btakita.agentdoc

import com.intellij.openapi.Disposable
import com.intellij.openapi.components.Service
import com.intellij.openapi.components.service
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.project.Project
import com.intellij.ui.EditorNotifications
import com.intellij.util.Alarm

/**
 * Per-project poller that flips [TurnStateBannerProvider] on and off as the CPC's
 * turn phase changes. Polls the `agent_doc_turn_projection` FFI (via
 * [TurnStateBridge]) for the selected markdown file and only asks the platform to
 * re-collect editor notifications when the phase actually transitions, so the
 * banner appears the moment a turn starts persisting and disappears when it goes
 * idle — without re-collecting on every tick.
 */
@Service(Service.Level.PROJECT)
class TurnStateBannerRefresher(private val project: Project) : Disposable {
    private val alarm = Alarm(Alarm.ThreadToUse.SWING_THREAD, this)
    private var lastState: String = ""

    fun start() {
        LOG.info("[turn-banner] refresher started; polling every ${POLL_MS}ms")
        schedule()
    }

    private fun schedule() {
        alarm.addRequest({
            poll()
            if (!project.isDisposed) schedule()
        }, POLL_MS)
    }

    private fun poll() {
        if (project.isDisposed) return
        val file = FileEditorManager.getInstance(project).selectedFiles
            .firstOrNull { it.name.endsWith(".md") }
        val state = if (file == null) "" else TurnStateBridge.presentationForFile(file.path).label
        if (state != lastState) {
            lastState = state
            LOG.info("[turn-banner] phase changed → \"${state.ifEmpty { "(idle, hidden)" }}\"; re-collecting banners")
            EditorNotifications.getInstance(project).updateAllNotifications()
        }
    }

    override fun dispose() {
        alarm.cancelAllRequests()
    }

    companion object {
        private const val POLL_MS = 1000
        private val LOG = Logger.getInstance(TurnStateBannerRefresher::class.java)
        fun getInstance(project: Project): TurnStateBannerRefresher = project.service()
    }
}
