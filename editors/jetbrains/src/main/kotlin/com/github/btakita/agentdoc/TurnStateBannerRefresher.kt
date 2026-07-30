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
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong

private const val TURN_STATE_CACHE_OBSERVE_INTERVAL_MS = 250L
private const val TURN_STATE_SLOW_PROJECTION_MS = 1_000L
private const val TURN_STATE_MAX_PATHS_PER_DRAIN = 4
private const val TURN_STATE_DRAIN_YIELD_MS = 50L
private const val TURN_STATE_AUTHORITY_SETTLE_MS = 75L

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
 * Per-project event loop that flips [TurnStateBannerProvider] on and off as the Project Controller
 * turn phase changes. Project Controller projection reads are queued only from IDE or agent-doc
 * events, then cached for banner/status-bar collection on the EDT.
 */
@Service(Service.Level.PROJECT)
class TurnStateBannerRefresher(private val project: Project) : Disposable {
    fun interface Listener {
        fun turnStateChanged()
    }

    private val executor = Executors.newSingleThreadScheduledExecutor { r ->
        Thread(r, "agent-doc-turn-state-events").apply { isDaemon = true }
    }
    private val started = AtomicBoolean(false)
    private val drainQueued = AtomicBoolean(false)
    private val pendingPaths = ConcurrentHashMap.newKeySet<String>()
    private val delayedPaths = ConcurrentHashMap.newKeySet<String>()
    private val openPaths = ConcurrentHashMap.newKeySet<String>()
    private val lastRefreshMs = ConcurrentHashMap<String, Long>()
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
        val delayMs = refreshDelayMs(filePath)
        if (delayMs > 0 && presentations.containsKey(filePath)) {
            scheduleDelayedRefresh(filePath, delayMs, reason)
            return
        }
        pendingPaths.add(filePath)
        scheduleDrain(reason)
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

    private fun scheduleDrain(reason: String, delayMs: Long) {
        if (!drainQueued.compareAndSet(false, true)) return
        val task = Runnable {
            try {
                drainPending(reason)
            } finally {
                drainQueued.set(false)
                if (pendingPaths.isNotEmpty()) {
                    scheduleDrain("rescheduled", TURN_STATE_DRAIN_YIELD_MS)
                }
            }
        }
        if (delayMs > 0) {
            executor.schedule(task, delayMs, TimeUnit.MILLISECONDS)
        } else {
            executor.execute(task)
        }
    }

    private fun scheduleDrain(reason: String) {
        scheduleDrain(reason, 0L)
    }

    private fun scheduleDelayedRefresh(filePath: String, delayMs: Long, reason: String) {
        if (!delayedPaths.add(filePath)) return
        val delayedReason = delayedReason(reason)
        executor.schedule(
            {
                delayedPaths.remove(filePath)
                if (project.isDisposed || !openPaths.contains(filePath)) return@schedule
                pendingPaths.add(filePath)
                scheduleDrain("$delayedReason-delayed")
            },
            delayMs,
            TimeUnit.MILLISECONDS,
        )
        LOG.debug("[turn-state] delayed refresh for $filePath by ${delayMs}ms via $delayedReason")
    }

    private fun refreshDelayMs(filePath: String): Long {
        val now = System.currentTimeMillis()
        val minIntervalUntil =
            (lastRefreshMs[filePath] ?: 0L) + TURN_STATE_CACHE_OBSERVE_INTERVAL_MS
        return (minIntervalUntil - now).coerceAtLeast(0L)
    }

    private fun drainPending(reason: String) {
        var inspected = 0
        while (!project.isDisposed && inspected < TURN_STATE_MAX_PATHS_PER_DRAIN) {
            val iterator = pendingPaths.iterator()
            if (!iterator.hasNext()) return
            val filePath = iterator.next()
            pendingPaths.remove(filePath)
            inspected++
            val delayMs = refreshDelayMs(filePath)
            if (delayMs > 0 && presentations.containsKey(filePath)) {
                scheduleDelayedRefresh(filePath, delayMs, backoffReason(reason))
                continue
            }
            refreshPath(filePath, reason)
        }
    }

    private fun refreshPath(filePath: String, reason: String) {
        val started = System.nanoTime()
        val projectRoot = TerminalUtil.resolveProjectPath(project.basePath, filePath).first
        val next =
            TurnStateBridge.presentationFromDocumentAuthority(
                filePath,
                NativeAdminControls.documentAuthority(projectRoot, filePath),
            )
        if (next == null) {
            scheduleDelayedRefresh(filePath, TURN_STATE_AUTHORITY_SETTLE_MS, "$reason-authority")
            return
        }
        val elapsedMs = TimeUnit.NANOSECONDS.toMillis(System.nanoTime() - started)
        val now = System.currentTimeMillis()
        lastRefreshMs[filePath] = now
        if (elapsedMs >= TURN_STATE_SLOW_PROJECTION_MS) {
            LOG.warn(
                "[turn-state] slow Project Controller projection for $filePath elapsed_ms=$elapsedMs"
            )
        }
        val previous = presentations.put(filePath, next)
        if (previous != next) {
            LOG.debug(
                "[turn-state] phase changed via $reason for $filePath: ${next.label.ifEmpty { "(idle, hidden)" }}"
            )
            notifyUi(filePath, reason)
        }
        scheduleDelayedRefresh(
            filePath,
            TURN_STATE_CACHE_OBSERVE_INTERVAL_MS,
            "$reason-authority-observe",
        )
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
        pendingPaths.clear()
        delayedPaths.clear()
        openPaths.clear()
        lastRefreshMs.clear()
        presentations.clear()
        transientStatuses.clearAll()
        listeners.clear()
        executor.shutdownNow()
    }

    companion object {
        private val LOG = Logger.getInstance(TurnStateBannerRefresher::class.java)

        fun getInstance(project: Project): TurnStateBannerRefresher = project.service()

        private fun isMarkdown(file: VirtualFile): Boolean = file.name.endsWith(".md")

        private fun backoffReason(reason: String): String = "${baseReason(reason)}-coalesced"

        private fun delayedReason(reason: String): String =
            if (reason.contains("-coalesced")) backoffReason(reason) else baseReason(reason)

        private fun baseReason(reason: String): String =
            reason.substringBefore("-coalesced").substringBefore("-delayed").ifBlank { "refresh" }
    }
}
