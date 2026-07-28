package com.github.btakita.agentdoc

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.fileEditor.ex.FileEditorManagerEx
import com.intellij.openapi.project.Project
import java.awt.AWTEvent
import java.awt.Container
import java.awt.event.ContainerEvent
import java.awt.event.AWTEventListener
import java.awt.Toolkit
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicReference
import javax.swing.SwingUtilities

/**
 * Detects editor layout changes (tab drags between splits, new splits, closed splits)
 * using one process-wide AWT event listener filtered to this project's
 * EditorsSplitters tree. A recursive ContainerListener used to attach to every
 * transient Swing container under the editor; long-running IDE sessions grew
 * that set into thousands of listeners and made every UI structure change
 * progressively more expensive.
 */
class LayoutChangeDetector(private val project: Project) {

    private val lastLayoutHash = AtomicReference<String?>(null)
    private val disposed = AtomicBoolean(false)
    private val fallbackGeneration = AtomicLong(0)
    private val listenerInstalled = AtomicBoolean(false)
    private val containerEventCount = java.util.concurrent.atomic.AtomicLong(0)
    private val editorsRoot = AtomicReference<Container?>(null)
    private val executor = Executors.newSingleThreadScheduledExecutor { runnable ->
        Thread(runnable, "agent-doc-layout-events").apply { isDaemon = true }
    }

    fun start() {
        // Resolve the splitters root before installing the filtered process listener.
        executor.schedule(init@{
            if (disposed.get()) return@init
            attachContainerListener()
        }, 2_000L, TimeUnit.MILLISECONDS)
    }

    fun dispose() {
        disposed.set(true)
        executor.shutdownNow()
        detachContainerEventListener()
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
                editorsRoot.set(root)
                if (listenerInstalled.compareAndSet(false, true)) {
                    Toolkit.getDefaultToolkit().addAWTEventListener(
                        containerEventListener,
                        AWTEvent.CONTAINER_EVENT_MASK,
                    )
                }
                LOG.info("[layout] one filtered AWT ContainerEvent listener attached to EditorsSplitters")
            } catch (e: Exception) {
                LOG.debug("[layout] Failed to attach filtered container listener: ${e.message}")
            }
        }
    }

    private val containerEventListener = AWTEventListener { event ->
        if (disposed.get() || project.isDisposed) return@AWTEventListener
        val containerEvent = event as? ContainerEvent ?: return@AWTEventListener
        val root = editorsRoot.get() ?: return@AWTEventListener
        val source = containerEvent.container
        val child = containerEvent.child
        val belongsToEditorTree =
            source === root ||
                SwingUtilities.isDescendingFrom(source, root) ||
                SwingUtilities.isDescendingFrom(child, root)
        if (!belongsToEditorTree) return@AWTEventListener

        val count = containerEventCount.incrementAndGet()
        if (count % 500 == 0L) {
            LOG.debug("[state] filteredContainerEvents=$count listeners=1")
        }
        val sourceToken =
            if (containerEvent.id == ContainerEvent.COMPONENT_ADDED) "containerAdd" else "containerRemove"
        scheduleSync(sourceToken)
    }

    private fun detachContainerEventListener() {
        if (!listenerInstalled.compareAndSet(true, false)) return
        val detach = Runnable {
            try {
                Toolkit.getDefaultToolkit().removeAWTEventListener(containerEventListener)
            } finally {
                editorsRoot.set(null)
            }
        }
        if (SwingUtilities.isEventDispatchThread()) {
            detach.run()
        } else {
            ApplicationManager.getApplication().invokeLater(detach)
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
            if (
                disposed.get() ||
                project.isDisposed ||
                !isCurrentGeneration(requestedGeneration)
            ) return@invokeLater
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
                // Structural changes and tab/focus changes are observations of
                // one editor surface. Sending them through one graph prevents
                // the legacy detector and the surface listener from racing two
                // independent full tmux reconciliations.
                EditorTabSyncListener.install(project).onEditorLayoutChanged(project)
            } catch (e: Exception) {
                LOG.debug("[layout] check failed: ${e.message}")
            }
        }
    }

    companion object {
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
