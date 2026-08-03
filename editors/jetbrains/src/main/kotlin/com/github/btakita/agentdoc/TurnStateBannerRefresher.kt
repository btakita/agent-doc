package com.github.btakita.agentdoc

import com.intellij.openapi.Disposable
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.components.Service
import com.intellij.openapi.components.service
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.FileEditorManagerEvent
import com.intellij.openapi.fileEditor.FileEditorManagerListener
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.ui.EditorNotifications
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong

internal class TransientDocumentStatus {
    private data class Entry(
        val token: Long,
        val presentation: TurnStateBridge.TurnStatePresentation,
    )

    private val nextToken = AtomicLong(0)
    private val entries = ConcurrentHashMap<String, Entry>()

    fun begin(filePath: String, label: String, tooltip: String? = null): Long {
        val token = nextToken.incrementAndGet()
        entries[filePath] =
            Entry(
                token,
                TurnStateBridge.TurnStatePresentation(
                    label = label,
                    guardPromptForwarding = false,
                    tooltip = tooltip,
                    showBanner = true,
                ),
            )
        return token
    }

    fun presentationFor(
        filePath: String,
        fallback: TurnStateBridge.TurnStatePresentation,
    ): TurnStateBridge.TurnStatePresentation = entries[filePath]?.presentation ?: fallback

    fun finish(filePath: String, token: Long): Boolean {
        val entry = entries[filePath] ?: return false
        return entry.token == token && entries.remove(filePath, entry)
    }

    fun clear(filePath: String) {
        entries.remove(filePath)
    }

    fun clearAll() {
        entries.clear()
    }
}

/**
 * Per-project reactive projection that flips [TurnStateBannerProvider] on and off as the Project
 * Controller turn phase changes. Each open markdown document retains one controller subscription;
 * banner/status-bar collection reads only the resulting cache on the EDT.
 */
@Service(Service.Level.PROJECT)
class TurnStateBannerRefresher(private val project: Project) : Disposable {
    fun interface Listener {
        fun turnStateChanged()
    }

    private val started = AtomicBoolean(false)
    private val openPaths = ConcurrentHashMap.newKeySet<String>()
    private val subscriptions = ConcurrentHashMap<String, CpTurnAuthoritySubscription>()
    private val presentations = ConcurrentHashMap<String, TurnStateBridge.TurnStatePresentation>()
    private val transientStatuses = TransientDocumentStatus()
    private val listeners = CopyOnWriteArrayList<Listener>()

    fun start() {
        if (!started.compareAndSet(false, true)) return
        LOG.info("[turn-state] event refresher started")
        project.messageBus
            .connect(this)
            .subscribe(
                FileEditorManagerListener.FILE_EDITOR_MANAGER,
                object : FileEditorManagerListener {
                    override fun selectionChanged(event: FileEditorManagerEvent) {
                        event.newFile?.let {
                            if (isMarkdown(it)) openPaths.add(it.path)
                            requestRefresh(it, "selection")
                        }
                        event.oldFile?.let { requestRefresh(it, "selection-old") }
                    }

                    override fun fileOpened(source: FileEditorManager, file: VirtualFile) {
                        if (isMarkdown(file)) openPaths.add(file.path)
                        requestRefresh(file, "file-opened")
                    }

                    override fun fileClosed(source: FileEditorManager, file: VirtualFile) {
                        if (!isMarkdown(file)) return
                        openPaths.remove(file.path)
                        subscriptions.remove(file.path)?.close()
                        presentations.remove(file.path)
                        transientStatuses.clear(file.path)
                        notifyUi(file.path, "file-closed")
                    }
                },
            )
        requestOpenMarkdownRefresh("startup")
    }

    fun addListener(listener: Listener): Disposable {
        listeners.add(listener)
        return Disposable { listeners.remove(listener) }
    }

    fun cachedPresentationFor(filePath: String): TurnStateBridge.TurnStatePresentation =
        transientStatuses.presentationFor(
            filePath,
            presentations[filePath] ?: TurnStateBridge.TurnStatePresentation("", false),
        )

    fun showTransientStatus(filePath: String, label: String, tooltip: String? = null): Long {
        val token = transientStatuses.begin(filePath, label, tooltip)
        notifyUi(filePath, "transient-status-start")
        return token
    }

    fun clearTransientStatus(filePath: String, token: Long) {
        if (!transientStatuses.finish(filePath, token)) return
        notifyUi(filePath, "transient-status-finish")
        requestRefresh(filePath, "transient-status-finish")
    }

    fun requestRefresh(file: VirtualFile, reason: String) {
        if (!isMarkdown(file) || project.isDisposed || !file.isValid) return
        requestRefresh(file.path, reason)
    }

    fun requestRefresh(filePath: String, reason: String) {
        if (project.isDisposed || !filePath.endsWith(".md")) return
        ensureSubscription(filePath, reason)
    }

    fun requestSelectedRefresh(reason: String) {
        ApplicationManager.getApplication().invokeLater {
            if (project.isDisposed) return@invokeLater
            FileEditorManager.getInstance(project).selectedFiles.filter(::isMarkdown).forEach {
                requestRefresh(it, reason)
            }
        }
    }

    private fun requestOpenMarkdownRefresh(reason: String) {
        ApplicationManager.getApplication().invokeLater {
            if (project.isDisposed) return@invokeLater
            FileEditorManager.getInstance(project).openFiles.filter(::isMarkdown).forEach {
                openPaths.add(it.path)
                requestRefresh(it, reason)
            }
            requestSelectedRefresh(reason)
        }
    }

    private fun ensureSubscription(filePath: String, reason: String) {
        if (subscriptions.containsKey(filePath)) return
        val projectRoot = TerminalUtil.resolveProjectPath(project.basePath, filePath).first
        val subscription =
            CpRouteClient.subscribeDocumentTurnAuthority(projectRoot, filePath) { authorityJson ->
                if (project.isDisposed || !openPaths.contains(filePath)) return@subscribeDocumentTurnAuthority
                val next =
                    TurnStateBridge.presentationFromDocumentAuthority(filePath, authorityJson)
                        ?: return@subscribeDocumentTurnAuthority
                val previous = presentations.put(filePath, next)
                if (previous != next) {
                    LOG.debug(
                        "[turn-state] projection changed via $reason for $filePath: " +
                            next.label.ifEmpty { "(idle, hidden)" },
                    )
                    notifyUi(filePath, "controller-authority-stream")
                }
            }
        val previous = subscriptions.putIfAbsent(filePath, subscription)
        if (previous != null) {
            subscription.close()
        }
    }

    private fun notifyUi(filePath: String, reason: String) {
        ApplicationManager.getApplication().invokeLater {
            if (project.isDisposed) return@invokeLater
            LocalFileSystem.getInstance().findFileByPath(filePath)?.let {
                EditorNotifications.getInstance(project).updateNotifications(it)
            }
            listeners.forEach { it.turnStateChanged() }
            LOG.debug("[turn-state] UI refreshed via $reason for $filePath")
        }
    }

    override fun dispose() {
        openPaths.clear()
        subscriptions.values.forEach(CpTurnAuthoritySubscription::close)
        subscriptions.clear()
        presentations.clear()
        transientStatuses.clearAll()
        listeners.clear()
    }

    companion object {
        private val LOG = Logger.getInstance(TurnStateBannerRefresher::class.java)

        fun getInstance(project: Project): TurnStateBannerRefresher = project.service()

        private fun isMarkdown(file: VirtualFile): Boolean = file.name.endsWith(".md")
    }
}
