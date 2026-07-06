package com.github.btakita.agentdoc

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.fileEditor.ex.FileEditorManagerEx
import com.intellij.openapi.project.Project
import java.awt.Container
import java.awt.event.ContainerEvent
import java.awt.event.ContainerListener
import java.io.File
import java.util.Collections
import java.util.WeakHashMap
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicReference

/**
 * Detects editor layout changes (tab drags between splits, new splits, closed splits)
 * using a ContainerListener on EditorsSplitters. The listener is attached
 * recursively with a weak de-dup set, so new split subtrees are covered by
 * Swing container events instead of a fallback polling thread.
 */
class LayoutChangeDetector(private val project: Project) {

    private val lastLayoutHash = AtomicReference<String?>(null)
    private val disposed = AtomicBoolean(false)
    private val fallbackSyncing = AtomicBoolean(false)
    private val fallbackGeneration = AtomicLong(0)
    private val listenerCount = java.util.concurrent.atomic.AtomicInteger(0)
    private val containerEventCount = java.util.concurrent.atomic.AtomicLong(0)
    private val executor = Executors.newSingleThreadScheduledExecutor { runnable ->
        Thread(runnable, "agent-doc-layout-events").apply { isDaemon = true }
    }
    // WeakHashMap so GC'd containers don't accumulate; synchronized for EDT access
    private val listenedContainers: MutableSet<Container> =
        Collections.synchronizedSet(Collections.newSetFromMap(WeakHashMap()))

    fun start() {
        // Attach ContainerListener to the splitters root (delayed — splitters may not exist yet)
        executor.schedule(init@{
            if (disposed.get()) return@init
            attachContainerListener()
        }, 2_000L, TimeUnit.MILLISECONDS)
    }

    fun dispose() {
        disposed.set(true)
        executor.shutdownNow()
        instances.remove(project)
    }

    private fun attachContainerListener() {
        // Must access Swing components on EDT
        ApplicationManager.getApplication().invokeLater {
            if (disposed.get() || project.isDisposed) return@invokeLater
            try {
                val managerEx = FileEditorManagerEx.getInstanceEx(project)
                val splitters = managerEx.splitters
                val root = splitters as? Container ?: return@invokeLater
                addRecursiveContainerListener(root)
                LOG.info("[layout] ContainerListener attached to EditorsSplitters")
            } catch (e: Exception) {
                LOG.debug("[layout] Failed to attach ContainerListener: ${e.message}")
            }
        }
    }

    private val containerListener = object : ContainerListener {
        override fun componentAdded(e: ContainerEvent) {
            val count = containerEventCount.incrementAndGet()
            (e.child as? Container)?.let { addRecursiveContainerListener(it) }
            if (count % 100 == 0L) LOG.info("[state] containerEvents=$count listeners=${listenerCount.get()}")
            scheduleSync("containerAdd")
        }
        override fun componentRemoved(e: ContainerEvent) {
            containerEventCount.incrementAndGet()
            scheduleSync("containerRemove")
        }
    }

    private fun addRecursiveContainerListener(container: Container) {
        if (!listenedContainers.add(container)) return // already attached — skip
        container.addContainerListener(containerListener)
        listenerCount.incrementAndGet()
        for (child in container.components) {
            if (child is Container) {
                addRecursiveContainerListener(child)
            }
        }
    }

    private fun nextGeneration(): Long =
        fallbackGeneration.incrementAndGet()

    private fun isCurrentGeneration(generation: Long): Boolean =
        fallbackGeneration.get() == generation

    private fun scheduleSync(source: String, delayMs: Long = 500L) {
        if (disposed.get()) return
        val myGen = nextGeneration()
        executor.schedule(sync@{
            if (disposed.get()) return@sync
            val isCurrent = isCurrentGeneration(myGen)
            if (!isCurrent) return@sync // superseded by newer event
            checkAndSync(source, myGen)
        }, delayMs.coerceAtLeast(0L), TimeUnit.MILLISECONDS)
    }

    private fun checkAndSync(source: String, requestedGeneration: Long) {
        if (disposed.get()) return
        // Read Swing component tree on EDT (thread-safe), then sync on background thread
        ApplicationManager.getApplication().invokeLater {
            if (disposed.get() || project.isDisposed) return@invokeLater
            try {
                val layout = LayoutDetector.detectEditorLayout(project)
                // Hash on structural shape only (column count + window count), NOT file contents.
                // File-content changes are caused by navigation between files and should NOT
                // trigger a sync — that would collapse the 2-pane layout when the user navigates
                // to a non-agent-doc file. Only structural changes (splits opened/closed, tab
                // drags that change window count) should trigger LayoutChangeDetector sync.
                // EditorTabSyncListener.selectionChanged handles .md file navigation separately.
                val manager = FileEditorManagerEx.getInstanceEx(project)
                val windowCount = manager.windows.size
                val hash = if (layout != null) {
                    "cols=${layout.columns.size},wins=$windowCount"
                } else {
                    "single,wins=$windowCount"
                }

                val prev = lastLayoutHash.getAndSet(hash)
                if (hash == prev) return@invokeLater // No change

                LOG.info("[layout] change detected via $source: $prev → $hash")

                val selectedFiles = manager.selectedFiles
                val visibleMdFiles = SyncLayoutAction.collectVisibleMarkdownFiles(selectedFiles)
                val focusedVFile = manager.selectedTextEditor?.virtualFile
                    ?.takeIf { it.name.endsWith(".md") }
                    ?: selectedFiles.firstOrNull { it.name.endsWith(".md") }
                if (visibleMdFiles.isEmpty() || focusedVFile == null) {
                    return@invokeLater
                }
                val focusedFile = focusedVFile.path
                val basePath = project.basePath
                val layoutSnapshot = layout
                executor.execute {
                    syncDetectedLayout(
                        source = source,
                        requestedGeneration = requestedGeneration,
                        basePath = basePath,
                        visibleMdFiles = visibleMdFiles,
                        focusedFile = focusedFile,
                        layout = layoutSnapshot,
                    )
                }
            } catch (e: Exception) {
                LOG.debug("[layout] check failed: ${e.message}")
            }
        }
    }

    private fun syncDetectedLayout(
        source: String,
        requestedGeneration: Long,
        basePath: String?,
        visibleMdFiles: List<String>,
        focusedFile: String,
        layout: EditorLayout?,
    ) {
        if (disposed.get() || project.isDisposed || !isCurrentGeneration(requestedGeneration)) return

        val focusedProjectRoot = resolveProjectRoot(basePath, focusedFile)
        val projectRoot = SyncLayoutAction.chooseSyncProjectRoot(
            basePath,
            focusedProjectRoot,
            visibleMdFiles,
        )
        val agentDoc = TerminalUtil.resolveAgentDoc(projectRoot)
        val syncLayout = SyncLayoutAction.absolutizeEditorLayout(
            projectRoot,
            SyncLayoutAction.normalizeEditorLayout(
                basePath,
                projectRoot,
                layout,
            ),
        )
        val cmd = SyncLayoutAction.buildSyncCommand(
            agentDoc = agentDoc,
            visibleMdFiles = visibleMdFiles,
            editorLayout = syncLayout,
            focusedFile = focusedFile,
            noAutostart = true,
            exactVisible = true,
        )

        val lib = AgentDocLib.get()
        val locked = lib?.agent_doc_sync_try_lock()
            ?: fallbackSyncing.compareAndSet(false, true)
        if (!locked) return

        var timedOut = false
        try {
            val result = SyncLayoutAction.runCommandWithTimeout(
                cmd,
                projectRoot,
            )
            timedOut = result.timedOut
            LOG.info(
                "[layout] sync exit=${result.exitCode} timedOut=${result.timedOut} cmd=${cmd.joinToString(" ")} output=${result.output.take(500)}"
            )
        } finally {
            lib?.agent_doc_sync_unlock() ?: fallbackSyncing.set(false)
            if (!isCurrentGeneration(requestedGeneration)) {
                val delayMs = if (timedOut) SYNC_TIMEOUT_REPLAY_DELAY_MS else 0L
                LOG.info("[layout] replaying latest structural sync request delayMs=$delayMs")
                scheduleSync("replay-$source", delayMs)
            }
        }
    }

    private fun resolveProjectRoot(basePath: String?, focusedFile: String): String {
        val ffi = NativePatching.resolveProjectPath(focusedFile)
        if (ffi != null) {
            if (basePath != null && ffi.first != basePath) {
                try {
                    PatchWatcher.getInstance(project).registerRoot(ffi.first)
                } catch (_: Exception) {
                    // Best-effort parity with TerminalUtil.resolveProject.
                }
            }
            return ffi.first
        }
        if (basePath != null) return basePath
        return File(focusedFile).parent ?: "/"
    }

    companion object {
        private const val SYNC_TIMEOUT_REPLAY_DELAY_MS = 5_000L
        private val LOG = Logger.getInstance(LayoutChangeDetector::class.java)
        private val instances = ConcurrentHashMap<Project, LayoutChangeDetector>()

        fun getInstance(project: Project): LayoutChangeDetector {
            return instances.computeIfAbsent(project) { p ->
                LayoutChangeDetector(p).also { it.start() }
            }
        }

        fun disposeProject(project: Project) {
            instances.remove(project)?.dispose()
        }
    }
}
