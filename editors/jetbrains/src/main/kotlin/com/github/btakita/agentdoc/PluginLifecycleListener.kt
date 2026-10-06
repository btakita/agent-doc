package com.github.btakita.agentdoc

import com.intellij.openapi.application.ApplicationActivationListener
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.Disposable
import com.intellij.openapi.editor.EditorFactory
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.FileEditorManagerEvent
import com.intellij.openapi.fileEditor.FileEditorManagerListener
import com.intellij.openapi.project.Project
import com.intellij.openapi.project.ProjectManager
import com.intellij.openapi.project.ProjectManagerListener
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.openapi.vfs.VirtualFileManager
import com.intellij.openapi.wm.IdeFrame
import com.intellij.openapi.util.Disposer
import java.util.LinkedHashMap
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference

/**
 * Owns per-project startup and cleanup for both project close and dynamic plugin unload.
 *
 * Registered in plugin.xml as a projectListener so IntelliJ manages the project lifecycle. The
 * application-level [PluginUnloadCleanupService] calls the same idempotent cleanup while open
 * projects survive a dynamic plugin replacement.
 */
class PluginLifecycleListener : ProjectManagerListener {
    override fun projectOpened(project: Project) {
        // Force creation of the application service whose Disposable boundary is plugin unload.
        ApplicationManager.getApplication().getService(PluginUnloadCleanupService::class.java)
        // `lifecycle` is this initialization's own scope, not the project service itself, so a
        // dynamic-unload release can drop every listener below and a later projectOpened -- the
        // still-live generation's restore after an aborted upgrade (GH #94) -- rebuilds them.
        val lifecycle =
            project.getService(ProjectPluginLifecycleService::class.java).beginInitialization() ?: return
        // Track document changes for typing debounce in SubmitAction
        EditorFactory.getInstance().eventMulticaster.addDocumentListener(TypingTracker, lifecycle)
        // Publish the live editor registration from a generation-owned listener.
        // XML project listeners are not reconstructed for projects that remain
        // open across a dynamic plugin replacement, which left native-save
        // routing pinned to the superseded plugin version even after the CRDT
        // replica had reattached from the replacement classloader.
        ReliableSyncLivenessListener.install(project, lifecycle)
        // Attach markdown buffers as CRDT replicas when the CP endpoint is available.
        CrdtReplicaManager.getInstance(project)
        // Registration is retained independently of the controller's current
        // durable-registration query. At startup that query can correctly be
        // empty before this editor has published its first open-document fact.
        CrdtReplicaManager.ensureOpenDocumentReplicas(project, "plugin-startup")
        // `#ctrlkillreregister` Tier 3: ask the controller which of THIS editor's
        // registrations it holds no replica for, and rebuild exactly those, so a
        // Lazily-retained response/backlog target cannot remain parked until an
        // operator focus change. Nothing is selected or focused.
        //
        // This replaces the blind re-register of every open markdown document. The
        // sweep dropped and rebuilt healthy CRDT baselines on every startup — the
        // lossiest operation the replica manager has — and still missed a stranded
        // registration whose document was not open in a tab. It remains the fallback
        // inside `pullMissingReplicas` for when the pull itself cannot be asked.
        CrdtReplicaManager.pullMissingReplicas(project, "plugin-startup")
        // Start watching for IPC patch files from agent-doc write --ipc
        val patchWatcher = PatchWatcher.getInstance(project)
        // Highlight agent-doc-specific markdown structures in the editor.
        VisualHighlighterManager.getInstance(project)
        // Detect editor layout changes (tab drags, new splits) and sync tmux
        LayoutChangeDetector.getInstance(project)
        // Flip the turn-state editor banner on/off as the CP turn phase changes.
        TurnStateBannerRefresher.getInstance(project).start()
        // Register EditorTabSyncListener via code (not XML) so it survives hot-reload
        val editorTabSync = EditorTabSyncListener.install(project)
        project.messageBus
            .connect(lifecycle)
            .subscribe(
                FileEditorManagerListener.FILE_EDITOR_MANAGER,
                editorTabSync,
        )
        // Install focus ingress before seeding the already-selected split. Dynamic reload and
        // project startup do not replay the event that selected it.
        EditorFocusSyncListener.install(project, editorTabSync)
        editorTabSync.onIdeActivated(project)
        // An i3 workspace switch can recreate or resize the embedded terminal surface without
        // changing editor selection/layout. Republish the settled editor surface when this IDE
        // frame becomes active so the normal controller projection repairs tmux automatically.
        ApplicationManager.getApplication()
            .messageBus
            .connect(lifecycle)
            .subscribe(
                ApplicationActivationListener.TOPIC,
                object : ApplicationActivationListener {
                    override fun applicationActivated(ideFrame: IdeFrame) {
                        if (ideFrame.project === project) {
                            editorTabSync.onIdeActivated(project)
                        }
                    }
                },
            )
        // Drive tmux pane focus on split-editor focus changes (#panefocussplit):
        // selectionChanged does not fire for focus movement between existing
        // splits, so this reuses editorTabSync's reconcile from focus events.
        // Mirror Project Controller-owned tmux focus back into editor selection.
        // TmuxPaneFocusSync keeps an actually focused editor authoritative, permits
        // embedded-terminal focus changes, and suppresses hidden cross-root targets.
        TmuxPaneFocusSync.install(project)
        project.messageBus
            .connect(lifecycle)
            .subscribe(
                FileEditorManagerListener.FILE_EDITOR_MANAGER,
                object : FileEditorManagerListener {
                    override fun selectionChanged(event: FileEditorManagerEvent) {
                        val file = event.newFile ?: return
                        if (file.name.endsWith(".md")) {
                            CrdtReplicaManager.requestRemoteDrain(project, file.path, "selection")
                        }
                    }

                    override fun fileOpened(source: FileEditorManager, file: VirtualFile) {
                        TypingTracker.scheduleOpenDocumentReport(file)
                        if (file.name.endsWith(".md")) {
                            patchWatcher.registerRootForFile(file.path)
                            CrdtReplicaManager.ensureOpenDocumentReplica(
                                project,
                                file.path,
                                "file-opened",
                            )
                            CrdtReplicaManager.requestRemoteDrain(project, file.path, "file-opened")
                        }
                    }

                    override fun fileClosed(source: FileEditorManager, file: VirtualFile) {
                        TypingTracker.clearOpenDocumentReport(file)
                    }
                },
            )
        EditorOpenFileSurface.snapshot(project)
            .asSequence()
            .filter { it.name.endsWith(".md") }
            .forEach { patchWatcher.registerRootForFile(it.path) }
        TypingTracker.reportOpenMarkdownDocuments(project)
        // Detect file renames/moves and update sessions.json path
        project.messageBus
            .connect(lifecycle)
            .subscribe(
                VirtualFileManager.VFS_CHANGES,
                FileRenameListener(project),
            )
    }

    companion object {
        private val LOG =
            com.intellij.openapi.diagnostic.Logger.getInstance(PluginLifecycleListener::class.java)

        /**
         * Rebuild plugin-owned project services after a dynamic package replacement.
         *
         * JetBrains keeps projects and editor tabs open across dynamic unload/load, so
         * [ProjectManagerListener.projectOpened] is not replayed for the replacement
         * classloader. The attach bridge calls this entry point after the new descriptor
         * is live, then waits for every eligible open document to own a new CRDT replica.
         *
         * `#jbupgradereattach`: returns a one-line receipt instead of raising on a
         * shortfall. Re-registration converges against whichever controller owns each
         * document's own project root, so it is not a property of this upgrade -- a
         * document from an unrelated open project can stay pending while the replacement
         * bytes are correct and live. Each pending path keeps its own bounded retry armed.
         */
        @JvmStatic
        fun initializeOpenProjectsAfterDynamicLoad(): String {
            check(!javax.swing.SwingUtilities.isEventDispatchThread()) {
                "dynamic plugin initialization must wait off the EDT"
            }
            val projects = java.util.concurrent.atomic.AtomicReference<List<Project>>(emptyList())
            ApplicationManager.getApplication().invokeAndWait {
                val openProjects = ProjectManager.getInstance().openProjects.filterNot { it.isDisposed }
                openProjects.forEach { project -> PluginLifecycleListener().projectOpened(project) }
                projects.set(openProjects)
            }
            val report = mergeReplicaRestartReports(
                projects.get().map { project ->
                    // Scoped per project so one project that cannot produce a receipt neither
                    // erases the others' nor reads as a converged reattach.
                    try {
                        CrdtReplicaManager.ensureOpenDocumentReplicasAndWait(
                            project,
                            "dynamic-plugin-load",
                        )
                    } catch (failure: Exception) {
                        val label = project.basePath ?: project.name
                        LOG.warn(
                            "[plugin-lifecycle] dynamic plugin load could not take a replica " +
                                "receipt for $label",
                            failure,
                        )
                        nativeReloadReplicaRestartReport(
                            listOf(label),
                            emptyList(),
                            liveProjects = 1,
                        )
                    }
                },
            )
            if (!report.converged) {
                LOG.warn(
                    "[plugin-lifecycle] dynamic plugin load left ${report.failedPaths.size} open " +
                        "document(s) awaiting replica re-registration: " +
                        "${report.failedPaths.joinToString()}; the replacement generation is live " +
                        "and each document keeps its own bounded retry armed",
                )
            }
            return dynamicLoadReattachReceipt(report)
        }

        /**
         * Stop every outgoing-generation listener before IntelliJ unloads its classloader.
         *
         * The system-classloader upgrade bridge calls this explicitly. Relying only on
         * application-service disposal is insufficient: JetBrains can retain a project
         * service/listener while replacing the plugin descriptor, leaving two CRDT managers
         * to observe the same Document and echo one another's remote projections.
         */
        @JvmStatic
        fun disposeOpenProjectsForDynamicUnload(): Int {
            check(javax.swing.SwingUtilities.isEventDispatchThread()) {
                "dynamic plugin cleanup must run on the EDT"
            }
            val projects = ProjectManager.getInstance().openProjects.filterNot { it.isDisposed }
            projects.forEach(::disposeProjectResources)
            // GH #94: also end each project's initialization scope, so the
            // listeners bound to it stop too and, if the upgrade then aborts with this generation
            // still loaded, initializeOpenProjectsAfterDynamicLoad can rebuild the project rather
            // than finding it already initialized and leaving its replica transport torn down.
            projects.forEach { project ->
                project.getServiceIfCreated(ProjectPluginLifecycleService::class.java)?.endInitialization()
            }
            return projects.size
        }

        internal fun disposeProjectResources(project: Project) {
            ReliableSyncLivenessListener.disposeProject(project)
            TurnStateBannerRefresher.disposeProject(project)
            CrdtReplicaManager.disposeProject(project)
            PatchWatcher.disposeProject(project)
            LayoutChangeDetector.disposeProject(project)
            VisualHighlighterManager.disposeProject(project)
            // Stop feeding focus events before releasing the surface graph, so a
            // late observation cannot re-create the root we are about to forget.
            EditorFocusSyncListener.disposeProject(project)
            EditorTabSyncListener.disposeProject(project)
            TmuxPaneFocusSync.disposeProject(project)
        }
    }

    override fun projectClosed(project: Project) {
        disposeProjectResources(project)
    }
}

/** Project-owned parent disposable for programmatic listeners installed at startup. */
class ProjectPluginLifecycleService(
    private val project: Project,
) : Disposable {
    private val scope = InitializationScope<Disposable>(
        open = { Disposer.newDisposable("agent-doc project lifecycle").also { Disposer.register(this, it) } },
        close = Disposer::dispose,
    )

    /** This initialization's listener scope, or null when the project is already initialized. */
    fun beginInitialization(): Disposable? = scope.begin()

    /** Drop the current initialization's listeners so a later [beginInitialization] succeeds. */
    fun endInitialization() {
        scope.end()
    }

    override fun dispose() {
        scope.forget()
        PluginLifecycleListener.disposeProjectResources(project)
    }
}

/**
 * One-at-a-time initialization scope (GH #94).
 *
 * The project service used to hold a one-shot `initialized` flag, so after a dynamic-unload
 * release the still-live generation could never re-initialize the project: an upgrade that then
 * aborted left the replica transport deregistered until the IDE restarted. A scope can be ended
 * and begun again; [end] closes the listeners bound to the scope it hands out.
 */
internal class InitializationScope<S : Any>(
    private val open: () -> S,
    private val close: (S) -> Unit,
) {
    private val current = AtomicReference<S?>(null)
    private val opening = AtomicBoolean(false)

    fun begin(): S? {
        if (!opening.compareAndSet(false, true)) return null
        return try {
            if (current.get() != null) null else open().also { current.set(it) }
        } finally {
            opening.set(false)
        }
    }

    fun end() {
        current.getAndSet(null)?.let(close)
    }

    /** The owner is disposing and closes the scope itself; just stop tracking it. */
    fun forget() {
        current.set(null)
    }

    fun isOpen(): Boolean = current.get() != null
}

/**
 * Plugin-owned application service whose disposal boundary is a dynamic plugin unload.
 *
 * Projects remain open while JetBrains swaps the plugin classloader, so their project-close
 * listeners do not run. Clearing every static per-project registry here prevents the old
 * classloader from being retained and lets the replacement generation initialize cleanly.
 */
class PluginUnloadCleanupService : Disposable {
    override fun dispose() {
        // Retire first: anything still queued in this classloader (a reload
        // intent, an EDT runnable) must find the generation retired before the
        // managers it would otherwise recreate are gone.
        PluginGeneration.retire()
        ProjectManager.getInstance().openProjects.forEach { project ->
            PluginLifecycleListener.disposeProjectResources(project)
        }
        // `#staleturnbanner`: strips outlive their unregistered provider, so remove them here.
        TurnStateBannerProvider.retireStrips()
        // `#jbclassloaderthreads`: application-global Kotlin objects belong to this plugin
        // classloader, not to a project. Dynamic unload used to leave their executors and the
        // native-library shutdown hook alive forever: after eleven upgrades IDEA owned 44 native
        // workers plus eleven current-document and socket-watchdog workers. Close only resources
        // that this generation actually created, and do it after project teardown has used them
        // for its final deregistration.
        PluginGeneration.closeResources()
    }
}

/**
 * `#pluginunloadresurrect`: whether this classloader's plugin generation has been
 * unloaded. A dynamic upgrade disposes the old generation's managers, but a
 * `reload_library` intent still reaching the old classloader ran its native-reload
 * restart, which recreated a `CrdtReplicaManager` there. That resurrected
 * generation kept attaching documents under the retired editor identity while
 * the replacement had published liveness under its own, so the controller
 * refused every registration as a stale endpoint and those documents stayed
 * detached until an IDE restart. Once retired, a generation never builds
 * replicas or reloads native code again.
 */
internal object PluginGeneration {
    private val resourceLock = Any()
    private val resources = LinkedHashMap<String, () -> Unit>()

    @Volatile
    var resourcesClosed: Boolean = false
        private set

    @Volatile
    var retired: Boolean = false
        private set

    fun retire() {
        retired = true
    }

    /** Register one application-global resource owned by this plugin classloader. */
    fun registerResource(name: String, close: () -> Unit) {
        val closeImmediately =
            synchronized(resourceLock) {
                if (resourcesClosed) {
                    true
                } else {
                    resources.putIfAbsent(name, close)
                    false
                }
            }
        if (closeImmediately) {
            close()
        }
    }

    /** Close in reverse creation order; every closer must be idempotent and non-throwing. */
    fun closeResources() {
        val closers = synchronized(resourceLock) {
            resourcesClosed = true
            resources.values.toList().asReversed().also { resources.clear() }
        }
        closers.forEach { close ->
            try {
                close()
            } catch (error: Throwable) {
                com.intellij.openapi.diagnostic.Logger
                    .getInstance(PluginGeneration::class.java)
                    .warn("[plugin-lifecycle] generation resource cleanup failed", error)
            }
        }
    }

    /** Tests share one classloader; production never un-retires a generation. */
    internal fun resetForTest() {
        retired = false
        resourcesClosed = false
    }
}
