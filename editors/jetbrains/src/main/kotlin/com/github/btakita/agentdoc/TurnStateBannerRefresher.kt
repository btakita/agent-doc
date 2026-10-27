package com.github.btakita.agentdoc

import com.intellij.notification.Notification
import com.intellij.notification.NotificationAction
import com.intellij.notification.NotificationGroupManager
import com.intellij.notification.NotificationType
import com.intellij.openapi.Disposable
import com.intellij.openapi.application.ApplicationManager
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
 * GH #323: which documents the turn banner tracks, and the controller subscription each one holds.
 *
 * A document is *displayed* once any editor surface shows it. On a JetBrains Remote Dev backend
 * the client's selection and open edges live in `ClientFileEditorManager` and are not published on
 * the project `FileEditorManager` topic (GH #262), so the banner provider's own collection edge
 * must admit the document: before this, a document opened in the client after the backend started
 * never entered the tracked set, every controller frame for it was discarded, and the banner stayed
 * hidden through an `awaiting_response` turn. A subscription opened by some other caller
 * (`requestRefresh`) for a document that is not yet displayed keeps its latest frame, so admission
 * renders the current phase instead of waiting for the next controller change.
 */
internal class TurnStateDocumentTracking<S : Any, F : Any>(
    private val subscribe: (path: String, reason: String) -> S,
    private val close: (S) -> Unit,
) {
    data class Admission<F>(val newlyAdmitted: Boolean, val replay: F?)

    private val displayed = ConcurrentHashMap.newKeySet<String>()
    private val subscriptions = ConcurrentHashMap<String, S>()
    private val heldFrames = ConcurrentHashMap<String, F>()

    /** Mark [path] displayed; a newly displayed path returns its held frame to apply now. */
    fun admit(path: String): Admission<F> {
        val newly = displayed.add(path)
        return Admission(newly, if (newly) heldFrames.remove(path) else null)
    }

    fun isDisplayed(path: String): Boolean = displayed.contains(path)

    fun hasSubscription(path: String): Boolean = subscriptions.containsKey(path)

    /** Open one subscription for [path]; returns true only when this call opened it. */
    fun ensureSubscription(path: String, reason: String): Boolean {
        if (subscriptions.containsKey(path)) return false
        val subscription = subscribe(path, reason)
        if (subscriptions.putIfAbsent(path, subscription) != null) {
            close(subscription)
            return false
        }
        return true
    }

    /**
     * Offer one controller frame. Returns true when it must be applied now (the document is
     * displayed). Otherwise the latest frame is held for [admit], unless the subscription was
     * already released, in which case a late frame is dropped so it can never resurrect a closed
     * document's phase.
     */
    fun offer(path: String, frame: F): Boolean {
        if (!subscriptions.containsKey(path)) return false
        heldFrames[path] = frame
        if (displayed.contains(path)) {
            // An admit racing this offer may already have replayed the same frame; applying a
            // frame twice is idempotent.
            heldFrames.remove(path, frame)
            return true
        }
        return false
    }

    /** Forget [path]: no editor in any client still shows it. Returns whether it was displayed. */
    fun release(path: String): Boolean {
        val was = displayed.remove(path)
        subscriptions.remove(path)?.let(close)
        heldFrames.remove(path)
        return was
    }

    fun clear() {
        displayed.clear()
        subscriptions.values.forEach(close)
        subscriptions.clear()
        heldFrames.clear()
    }
}

/**
 * Per-project reactive projection that flips [TurnStateBannerProvider] on and off as the Project
 * Controller turn phase changes. Each open markdown document retains one controller subscription;
 * banner/status-bar collection reads only the resulting cache on the EDT.
 */
class TurnStateBannerRefresher(private val project: Project) : Disposable {
    fun interface Listener {
        fun turnStateChanged()
    }

    private val started = AtomicBoolean(false)
    private val tracking =
        TurnStateDocumentTracking<CpTurnAuthoritySubscription, String>(
            subscribe = ::subscribe,
            close = CpTurnAuthoritySubscription::close,
        )
    private val presentations = ConcurrentHashMap<String, TurnStateBridge.TurnStatePresentation>()
    // `#componentownclose`: the last controller frame per document, so a diagnostic the
    // binary wrote between frames (the marker check at edit time) renders without waiting
    // for the next frame.
    private val lastAuthority = ConcurrentHashMap<String, String>()
    private val transientStatuses = TransientDocumentStatus()
    private val inputRequiredNotifications =
        InputRequiredNotifications<Notification> { it.expire() }
    private val deferredClearSupersededNotices = DeferredClearSupersededNotices()
    private val listeners = CopyOnWriteArrayList<Listener>()

    fun start() {
        if (!started.compareAndSet(false, true)) return
        LOG.info("[turn-state] event refresher started")
        GenerationListeners.subscribe(
            project,
            FileEditorManagerListener.FILE_EDITOR_MANAGER,
            object : FileEditorManagerListener {
                override fun selectionChanged(event: FileEditorManagerEvent) {
                    event.newFile?.let { observeDisplayed(it, "selection") }
                    event.oldFile?.let { requestRefresh(it, "selection-old") }
                }

                override fun fileOpened(source: FileEditorManager, file: VirtualFile) {
                    observeDisplayed(file, "file-opened")
                }

                override fun fileClosed(source: FileEditorManager, file: VirtualFile) {
                    if (!isMarkdown(file)) return
                    // GH #323 / GH #241: `fileClosed` fires when ONE editor is removed. A
                    // Remote Dev client can still show the document, and no selection edge
                    // reaches this listener to re-admit it, so release only when no client
                    // still has it open.
                    DocumentCloseDecision.route("turn-state-banner", project, source, file) {
                        release(file.path)
                    }
                }
            },
            this,
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
        tracking.ensureSubscription(filePath, reason)
    }

    /**
     * An editor surface shows [file]: track it and make sure it is subscribed. Called from the
     * banner provider's collection edge (every client, including Remote Dev) as well as from the
     * project editor-manager events. Cheap and idempotent on the calling thread; subscription
     * setup and any held-frame replay run on a pooled thread.
     */
    fun observeDisplayed(file: VirtualFile, reason: String) {
        if (!isMarkdown(file) || project.isDisposed || !file.isValid) return
        val filePath = file.path
        val admission = tracking.admit(filePath)
        if (!admission.newlyAdmitted && tracking.hasSubscription(filePath)) return
        if (admission.newlyAdmitted) {
            LOG.info("[turn-state] tracking $filePath via $reason")
        }
        ApplicationManager.getApplication().executeOnPooledThread {
            if (project.isDisposed || !tracking.isDisplayed(filePath)) return@executeOnPooledThread
            tracking.ensureSubscription(filePath, reason)
            admission.replay?.let { applyAuthority(filePath, it, "$reason-replay") }
        }
    }

    /**
     * Re-render [filePath] from its last controller frame. The status label reads the binary's
     * `editor-route-errors` diagnostic only while rendering a frame, so a diagnostic written
     * between frames (`#componentownclose`) needs this to show at edit time. No-op until the
     * first frame arrives or once the document is released.
     */
    fun reapplyLastAuthority(filePath: String, reason: String) {
        if (project.isDisposed || !tracking.isDisplayed(filePath)) return
        ApplicationManager.getApplication().executeOnPooledThread {
            if (project.isDisposed) return@executeOnPooledThread
            val authorityJson = lastAuthority[filePath] ?: return@executeOnPooledThread
            applyPresentation(filePath, authorityJson, reason, record = false)
        }
    }

    private fun release(filePath: String) {
        if (tracking.release(filePath)) {
            LOG.info("[turn-state] released $filePath: closed in every client")
        }
        presentations.remove(filePath)
        lastAuthority.remove(filePath)
        transientStatuses.clear(filePath)
        inputRequiredNotifications.clear(filePath)
        deferredClearSupersededNotices.forget(filePath)
        notifyUi(filePath, "file-closed")
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
                observeDisplayed(it, reason)
            }
            requestSelectedRefresh(reason)
        }
    }

    private fun subscribe(filePath: String, reason: String): CpTurnAuthoritySubscription {
        val projectRoot = TerminalUtil.resolveProjectPath(project.basePath, filePath).first
        return CpRouteClient.subscribeDocumentTurnAuthority(projectRoot, filePath) { authorityJson ->
            if (project.isDisposed || !tracking.offer(filePath, authorityJson)) {
                return@subscribeDocumentTurnAuthority
            }
            applyAuthority(filePath, authorityJson, reason)
        }
    }

    private fun applyAuthority(filePath: String, authorityJson: String, reason: String) {
        if (project.isDisposed) return
        // `#clearsupersedenotice`: the supervisor dropped the operator's queued clear long
        // after `session clear` exited; this stream is the only path that reports it.
        TurnStateBridge.deferredClearSupersededObservation(authorityJson)?.let { observation ->
            val dropped = observation.notice
            if (deferredClearSupersededNotices.observe(filePath, dropped) && dropped != null) {
                notifyDeferredClearSuperseded(filePath, dropped)
            }
        }
        applyPresentation(filePath, authorityJson, reason, record = true)
    }

    /**
     * Render one controller frame into the cached presentation. [record] stores it as the
     * document's last frame; a re-render ([reapplyLastAuthority]) passes false and is dropped if a
     * newer frame landed meanwhile, so it can never paint an older phase over a newer one.
     */
    private fun applyPresentation(
        filePath: String,
        authorityJson: String,
        reason: String,
        record: Boolean,
    ) {
        val next = TurnStateBridge.presentationFromDocumentAuthority(filePath, authorityJson)
        val previous =
            synchronized(lastAuthority) {
                if (record) {
                    lastAuthority[filePath] = authorityJson
                } else if (lastAuthority[filePath] !== authorityJson) {
                    return
                }
                next ?: return
                presentations.put(filePath, next)
            }
        if (next != null && previous != next) {
            // GH #323: INFO, once per change per document, so a missing banner can be matched
            // against the controller's `turn_in_flight` for that document.
            LOG.info(
                "[turn-state] projection changed via $reason for $filePath: " +
                    next.label.ifEmpty { "(idle, hidden)" } + " banner=${next.showBanner}",
            )
            // `#jbinputrequiredstale`: the balloon tracks the CURRENT state, so it is
            // retracted the moment the document stops requiring input. Leaving it up
            // turned every answered prompt into a standing false positive, and every
            // later edge stacked another balloon beside the stale one.
            if (next.inputRequired) {
                if (previous?.inputRequired != true) {
                    notifyInputRequired(filePath)
                }
            } else {
                inputRequiredNotifications.clear(filePath)
            }
            notifyUi(filePath, "controller-authority-stream")
        }
    }

    private fun notifyInputRequired(filePath: String) {
        inputRequiredNotifications.raise(filePath) { buildInputRequiredNotification(filePath) }
    }

    private fun buildInputRequiredNotification(filePath: String): Notification {
        val fileName = java.io.File(filePath).name
        val notification =
            NotificationGroupManager.getInstance()
                .getNotificationGroup("Agent Doc")
                .createNotification(
                    "Agent input required for $fileName",
                    "Open the Agent Doc terminal to answer the approval prompt.",
                    NotificationType.WARNING,
                )
        notification.isImportant = true
        notification.addAction(
            NotificationAction.createSimple("Focus Agent Terminal") {
                // The notification is document-scoped, so publish that document through the
                // existing editor-focus command plane before activating the shared terminal tab.
                // The controller then selects the pane/window that owns this session document.
                if (!EditorFocusSyncListener.routeDocumentFocus(project, filePath)) {
                    LOG.warn("[turn-state] unable to route terminal focus for $filePath")
                }
                // `#jbfocusnoop`: this used to call a Unit-returning focus that
                // `return`ed on every miss, so a click with no live agent-doc tab
                // navigated nowhere and told the operator nothing. Report the miss.
                when (IdeTerminalHost.focusExisting(project)) {
                    TerminalFocusOutcome.AGENT_TAB -> Unit
                    TerminalFocusOutcome.TOOL_WINDOW_ONLY ->
                        LOG.info(
                            "[turn-state] Focus Agent Terminal: no \"agent-doc\" tab in the " +
                                "terminal tool window; activated the tool window instead",
                        )
                    TerminalFocusOutcome.NOTHING ->
                        NotificationGroupManager.getInstance()
                            .getNotificationGroup("Agent Doc")
                            .createNotification(
                                "No terminal to focus",
                                "This IDE has no Terminal tool window, so there is no " +
                                    "agent-doc terminal to open. Answer the prompt in the " +
                                    "tmux pane running the session.",
                                NotificationType.WARNING,
                            )
                            .notify(project)
                }
            },
        )
        notification.notify(project)
        return notification
    }

    private fun notifyDeferredClearSuperseded(filePath: String, dropped: DeferredClearSuperseded) {
        val relativePath = TerminalUtil.resolveProjectPath(project.basePath, filePath).second
        LOG.info(
            "[turn-state] queued clear dropped for $filePath clear_epoch=${dropped.clearEpoch} " +
                "pane=${dropped.pane} reason=${dropped.reason} (#clearsupersedenotice)",
        )
        ApplicationManager.getApplication().invokeLater {
            if (project.isDisposed) return@invokeLater
            val notification =
                NotificationGroupManager.getInstance()
                    .getNotificationGroup("Agent Doc")
                    .createNotification(
                        "Queued clear not sent",
                        TerminalUtil.notificationHtml(
                            TerminalUtil.buildDeferredClearSupersededMessage(relativePath, dropped),
                        ),
                        NotificationType.INFORMATION,
                    )
            notification.addAction(
                NotificationAction.createSimpleExpiring("Clear Session Context") {
                    val file = LocalFileSystem.getInstance().findFileByPath(filePath)
                    if (file != null && !project.isDisposed) {
                        TerminalUtil.clearSessionContext(project, file)
                    }
                },
            )
            notification.notify(project)
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
        tracking.clear()
        presentations.clear()
        lastAuthority.clear()
        transientStatuses.clearAll()
        inputRequiredNotifications.clearAll()
        deferredClearSupersededNotices.clearAll()
        listeners.clear()
    }

    companion object {
        private val LOG = Logger.getInstance(TurnStateBannerRefresher::class.java)
        private val instances = ConcurrentHashMap<Project, TurnStateBannerRefresher>()

        fun getInstance(project: Project): TurnStateBannerRefresher =
            instances.computeIfAbsent(project, ::TurnStateBannerRefresher)

        fun disposeProject(project: Project) {
            instances.remove(project)?.dispose()
        }

        private fun isMarkdown(file: VirtualFile): Boolean = file.name.endsWith(".md")
    }
}
