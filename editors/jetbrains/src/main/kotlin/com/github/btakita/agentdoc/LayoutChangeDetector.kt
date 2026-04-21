package com.github.btakita.agentdoc

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.fileEditor.ex.FileEditorManagerEx
import com.intellij.openapi.project.Project
import java.awt.Container
import java.awt.event.ContainerEvent
import java.awt.event.ContainerListener
import java.util.Collections
import java.util.WeakHashMap
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference

/**
 * Detects editor layout changes (tab drags between splits, new splits, closed splits)
 * using a hybrid approach:
 *
 * 1. ContainerListener on EditorsSplitters — catches Swing tree changes immediately
 * 2. Fallback poll (5s) — safety net for edge cases events miss
 *
 * Both paths compute a layout hash and only sync when it changes.
 */
class LayoutChangeDetector(private val project: Project) {

    private val lastLayoutHash = AtomicReference<String?>(null)
    private val disposed = AtomicBoolean(false)
    private val fallbackSyncing = AtomicBoolean(false)
    private val fallbackGeneration = java.util.concurrent.atomic.AtomicLong(0)
    private var pollThread: Thread? = null
    private val listenerCount = java.util.concurrent.atomic.AtomicInteger(0)
    private val containerEventCount = java.util.concurrent.atomic.AtomicLong(0)
    // WeakHashMap so GC'd containers don't accumulate; synchronized for EDT access
    private val listenedContainers: MutableSet<Container> =
        Collections.synchronizedSet(Collections.newSetFromMap(WeakHashMap()))

    fun start() {
        // Attach ContainerListener to the splitters root (delayed — splitters may not exist yet)
        Thread({
            Thread.sleep(2000) // Wait for editor to initialize
            if (disposed.get()) return@Thread
            attachContainerListener()
            startFallbackPoll()
        }, "agent-doc-layout-detector-init").apply {
            isDaemon = true
            start()
        }
    }

    fun dispose() {
        disposed.set(true)
        pollThread?.interrupt()
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
            // Do NOT recurse into the new child's sub-tree here. The initial
            // one-shot walk from attachContainerListener covers the tree that
            // exists at plugin start; the 5s fallback poll (checkAndSync)
            // picks up any structural change afterwards. Recursive re-attach
            // on every add was the source of the listener-count blow-up
            // (listeners climbing past 1500 in minutes on heavy Swing churn).
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

    private fun startFallbackPoll() {
        pollThread = Thread({
            var pollCount = 0L
            while (!disposed.get()) {
                try {
                    Thread.sleep(POLL_INTERVAL_MS)
                    if (!disposed.get()) {
                        checkAndSync("poll")
                        pollCount++
                        if (pollCount % 12 == 0L) { // ~every 60s at 5s interval
                            LOG.info("[state] layout-detector: listeners=${listenerCount.get()} containerEvents=${containerEventCount.get()}")
                        }
                    }
                } catch (_: InterruptedException) {
                    break
                }
            }
        }, "agent-doc-layout-poll").apply {
            isDaemon = true
            start()
        }
    }

    private fun scheduleSync(source: String) {
        if (disposed.get()) return
        // Debounce via FFI (shared across editors), fallback to local counter
        val lib = AgentDocLib.get()
        val myGen = lib?.agent_doc_sync_bump_generation() ?: fallbackGeneration.incrementAndGet()
        Thread({
            Thread.sleep(500)
            if (disposed.get()) return@Thread
            val isCurrent = lib?.agent_doc_sync_check_generation(myGen)
                ?: (fallbackGeneration.get() == myGen)
            if (!isCurrent) return@Thread // superseded by newer event
            checkAndSync(source)
        }, "agent-doc-layout-sync-$source").apply {
            isDaemon = true
            start()
        }
    }

    private fun checkAndSync(source: String) {
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
                val windowCount = FileEditorManagerEx.getInstanceEx(project).windows.size
                val hash = if (layout != null) {
                    "cols=${layout.columns.size},wins=$windowCount"
                } else {
                    "single,wins=$windowCount"
                }

                val prev = lastLayoutHash.getAndSet(hash)
                if (hash == prev) return@invokeLater // No change

                LOG.info("[layout] change detected via $source: $prev → $hash")

                val lib = AgentDocLib.get()
                val locked = lib?.agent_doc_sync_try_lock()
                    ?: fallbackSyncing.compareAndSet(false, true)
                if (!locked) return@invokeLater
                // CLI call on background thread to avoid blocking EDT
                Thread({
                    try {
                        SyncLayoutAction.syncLayout(project, notify = false)
                    } finally {
                        lib?.agent_doc_sync_unlock() ?: fallbackSyncing.set(false)
                    }
                }, "agent-doc-layout-sync-$source").apply {
                    isDaemon = true
                    start()
                }
            } catch (e: Exception) {
                LOG.debug("[layout] check failed: ${e.message}")
            }
        }
    }

    companion object {
        private const val POLL_INTERVAL_MS = 5000L
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
