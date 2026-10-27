package com.github.btakita.agentdoc

import com.intellij.openapi.Disposable
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.diagnostic.ControlFlowException
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.editor.EditorFactory
import com.intellij.openapi.editor.event.EditorFactoryEvent
import com.intellij.openapi.editor.event.EditorFactoryListener
import com.intellij.openapi.editor.event.EditorMouseEvent
import com.intellij.openapi.editor.event.EditorMouseListener
import com.intellij.openapi.editor.ex.EditorEx
import com.intellij.openapi.editor.ex.FocusChangeListener
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.FileEditorManagerEvent
import com.intellij.openapi.fileEditor.FileEditorManagerListener
import com.intellij.openapi.fileEditor.ex.FileEditorManagerEx
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.Disposer
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.openapi.wm.IdeFocusManager
import java.awt.AWTEvent
import java.awt.Component
import java.awt.Container
import java.awt.KeyboardFocusManager
import java.awt.Toolkit
import java.awt.event.AWTEventListener
import java.awt.event.FocusEvent
import java.awt.event.MouseEvent
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicReference
import javax.swing.SwingUtilities

/**
 * Latest-wins settled selection probe used by editor-tree mouse ingress.
 *
 * IDEA applies a tab/split click over an unbounded number of EDT turns while its focus manager is
 * still transferring ownership. The clicked component cannot be trusted to identify the
 * destination document, so the callback reads the authoritative selected file only after IDEA
 * reports settled focus. A newer press or disposal makes every older callback inert.
 */
internal class SettledEditorTreeFocusProbe<T>(
    private val scheduleWhenFocusSettles: ((() -> Unit) -> Unit),
    private val selectedValue: () -> T?,
    private val isActive: () -> Boolean,
    private val isEligible: (T) -> Boolean,
    private val emit: (T) -> Unit,
) {
    private val generation = AtomicLong(0)
    private val closed = AtomicBoolean(false)

    fun observeMousePress(belongsToEditorTree: Boolean) {
        if (closed.get() || !belongsToEditorTree) return
        val requestedGeneration = generation.incrementAndGet()
        scheduleWhenFocusSettles {
            if (
                closed.get() ||
                    generation.get() != requestedGeneration ||
                    !isActive()
            ) {
                return@scheduleWhenFocusSettles
            }
            val selected = selectedValue() ?: return@scheduleWhenFocusSettles
            if (isEligible(selected)) emit(selected)
        }
    }

    fun close() {
        closed.set(true)
        generation.incrementAndGet()
    }
}

/**
 * Reads the project's editor split tree, or `null` while the platform has not built it yet.
 *
 * `FileEditorManagerImpl.mainSplitters` is a `lateinit` assigned when the project frame's editor
 * area is created. On IntelliJ 2026.3 (263) `ProjectManagerListener.projectOpened` can run before
 * that, and `getSplitters()` then throws [UninitializedPropertyAccessException] from inside the
 * dock-container lookup. That one exception is the platform's "not ready yet"
 * answer, so it maps to `null`; every other failure still propagates.
 */
internal object EditorSplittersReadiness {
    fun <T : Any> resolveOrNull(read: () -> T?): T? =
        try {
            read()
        } catch (notReady: UninitializedPropertyAccessException) {
            null
        }
}

/**
 * One-shot attachment of the editor-tree listener, deferred until the editor split tree exists.
 *
 * [tryAttach] is called once at install and again from each readiness signal. It attaches at most
 * once, never throws for the not-yet-built split tree, and is inert after [close].
 */
internal class DeferredEditorTreeAttachment<R : Any>(
    private val resolveRoot: () -> R?,
    private val isActive: () -> Boolean,
    private val attach: (R) -> Unit,
) {
    private val attached = AtomicBoolean(false)
    private val closed = AtomicBoolean(false)

    val isAttached: Boolean
        get() = attached.get()

    /** Returns true once the listener is attached, false while the split tree is not built yet. */
    fun tryAttach(): Boolean {
        if (attached.get()) return true
        if (closed.get() || !isActive()) return false
        val root = EditorSplittersReadiness.resolveOrNull(resolveRoot) ?: return false
        if (!attached.compareAndSet(false, true)) return true
        attach(root)
        return true
    }

    fun close() {
        closed.set(true)
    }
}

/**
 * Builds and starts one listener without letting its failure escape into the caller.
 *
 * `PluginLifecycleListener.projectOpened` installs focus sync in the middle of project startup.
 * An exception from it aborted every later startup step. A failed start is
 * discarded, reported, and not recorded, so a later install can try again. Platform control-flow
 * exceptions (cancellation) still propagate.
 */
internal fun <T : Any> startIsolated(
    create: () -> T,
    start: (T) -> Unit,
    discard: (T) -> Unit,
    report: (Throwable) -> Unit,
): T? {
    val instance = create()
    try {
        start(instance)
    } catch (failure: RuntimeException) {
        discard(instance)
        if (failure is ControlFlowException) throw failure
        report(failure)
        return null
    }
    return instance
}

/**
 * Drives tmux pane focus when the operator moves editor focus between split
 * editor windows (`#panefocussplit`).
 *
 * [EditorTabSyncListener] reconciles the tmux active pane on
 * [com.intellij.openapi.fileEditor.FileEditorManagerListener.selectionChanged],
 * which fires for tab / visible-file-set changes but NOT for focus movement
 * between two already-open split editors. When two agent-doc documents are open
 * side by side (a 2-column editor split mirrored as two co-visible tmux panes),
 * clicking into the other split never fired a reconcile, so the tmux active
 * pane did not follow the editor selection.
 *
 * This listener closes that gap: it attaches per-editor focus listeners, an editor-event mouse
 * listener, and one project-filtered AWT listener covering the entire editor split tree (including
 * tab chrome and custom editors), then routes activation back through the same debounced,
 * generation-guarded reconcile in [EditorTabSyncListener].
 *
 * Thin-plugin contract: this only reports the focus event. All debounce / dedup
 * / tmux-targeting logic stays in [EditorTabSyncListener] and the `agent-doc`
 * CLI, not here.
 */
class EditorFocusSyncListener private constructor(
    private val project: Project,
    private val tabSync: EditorTabSyncListener,
) : Disposable {
    private val disposed = AtomicBoolean(false)
    private val listenerInstalled = AtomicBoolean(false)
    private val editorsRoot = AtomicReference<Container?>(null)
    // GH #339 R1 Tier A: the readiness subscription is staged under its own disposable, which
    // stands in for the connection the direct subscription used to disconnect.
    private val readinessConnection = AtomicReference<Disposable?>(null)

    private val editorTreeAttachment =
        DeferredEditorTreeAttachment(
            resolveRoot = { FileEditorManagerEx.getInstanceEx(project).splitters as? Container },
            isActive = { !disposed.get() && !project.isDisposed },
            attach = ::installEditorTreeListener,
        )

    private val settledTreeFocusProbe =
        SettledEditorTreeFocusProbe<VirtualFile>(
            scheduleWhenFocusSettles = { callback ->
                IdeFocusManager.getInstance(project).doWhenFocusSettlesDown(Runnable(callback))
            },
            selectedValue = {
                FileEditorManagerEx.getInstanceEx(project).currentWindow?.selectedFile
            },
            isActive = {
                !disposed.get() &&
                    !project.isDisposed &&
                    JetBrainsEditorSurfaces.isProjectSurfaceActive(project)
            },
            isEligible = AgentDocSessionFiles::isSessionDocument,
            emit = { file -> tabSync.onEditorFocusGained(project, file) },
        )

    private val editorTreeMouseListener = AWTEventListener { event ->
        if (disposed.get() || project.isDisposed) return@AWTEventListener
        val component =
            when (event) {
                is MouseEvent -> {
                    if (event.id != MouseEvent.MOUSE_PRESSED) return@AWTEventListener
                    event.source as? Component
                }

                is FocusEvent -> {
                    if (event.id != FocusEvent.FOCUS_GAINED) return@AWTEventListener
                    event.component
                }

                else -> null
            } ?: return@AWTEventListener
        // Editor splitters can be replaced while restoring a layout or reloading the plugin.
        // Resolve the current tree at the event edge instead of trusting the startup component.
        val root =
            EditorSplittersReadiness.resolveOrNull {
                FileEditorManagerEx.getInstanceEx(project).splitters as? Container
            }
                ?: editorsRoot.get()
                ?: return@AWTEventListener
        editorsRoot.set(root)
        val belongsToEditorTree =
            JetBrainsEditorSurfaces.belongsToAnyEditorSurface(project, component) ||
                component === root || SwingUtilities.isDescendingFrom(component, root)
        settledTreeFocusProbe.observeMousePress(belongsToEditorTree)
    }

    private val focusListener = object : FocusChangeListener {
        override fun focusGained(editor: Editor) = handleEditorActivated(editor)

        override fun focusGained(editor: Editor, event: FocusEvent) = handleEditorActivated(editor)
    }

    private val mouseListener = object : EditorMouseListener {
        override fun mousePressed(event: EditorMouseEvent) = handleEditorActivated(event.editor)
    }

    private fun start() {
        val factory = EditorFactory.getInstance()
        // The event multicaster observes every current and future editor. A
        // per-editor mouse listener can miss a restored split when its editor
        // was created during project/plugin startup before attachment completed
        // (`#panefocussplit`). Project filtering remains in the event handler.
        GenerationListeners.addEditorMouseListener(mouseListener, this)
        GenerationListeners.addEditorFactoryListener(
            object : EditorFactoryListener {
                override fun editorCreated(event: EditorFactoryEvent) = attach(event.editor)
            },
            this,
        )
        // Attach to editors already open when the project (or a hot-reloaded
        // plugin) starts, so focus sync works without reopening files.
        for (editor in factory.allEditors) {
            attach(editor)
        }
        attachEditorTreeMouseListener()
    }

    private fun attachEditorTreeMouseListener() {
        onEdt {
            // `projectOpened` can run before the editor area exists. Wait for the
            // first editor event instead of failing: a tab cannot open or change selection before
            // the split tree is built, so either event proves it is ready.
            if (!editorTreeAttachment.tryAttach()) awaitEditorSplitters()
        }
    }

    private fun awaitEditorSplitters() {
        if (disposed.get() || project.isDisposed) return
        val connection = Disposer.newDisposable("agent-doc editor-tree readiness")
        Disposer.register(this, connection)
        if (!readinessConnection.compareAndSet(null, connection)) {
            Disposer.dispose(connection)
            return
        }
        GenerationListeners.subscribe(
            project,
            FileEditorManagerListener.FILE_EDITOR_MANAGER,
            object : FileEditorManagerListener {
                override fun fileOpened(source: FileEditorManager, file: VirtualFile) =
                    retryEditorTreeAttachment()

                override fun selectionChanged(event: FileEditorManagerEvent) =
                    retryEditorTreeAttachment()
            },
            connection,
        )
    }

    private fun retryEditorTreeAttachment() {
        onEdt {
            if (editorTreeAttachment.tryAttach()) {
                readinessConnection.getAndSet(null)?.let(Disposer::dispose)
            }
        }
    }

    private fun installEditorTreeListener(root: Container) {
        editorsRoot.set(root)
        if (listenerInstalled.compareAndSet(false, true)) {
            Toolkit.getDefaultToolkit()
                .addAWTEventListener(
                    editorTreeMouseListener,
                    AWTEvent.MOUSE_EVENT_MASK or AWTEvent.FOCUS_EVENT_MASK,
                )
        }
        // Hot reload and project startup do not replay the focus event that established the
        // already-selected split. Seed that existing focus through the same settled,
        // generation-fenced ingress instead of waiting for the operator to click again.
        val focusOwner =
            KeyboardFocusManager.getCurrentKeyboardFocusManager().permanentFocusOwner
        settledTreeFocusProbe.observeMousePress(
            focusOwner != null &&
                (JetBrainsEditorSurfaces.belongsToAnyEditorSurface(project, focusOwner) ||
                    focusOwner === root || SwingUtilities.isDescendingFrom(focusOwner, root)),
        )
    }

    private fun onEdt(action: () -> Unit) {
        if (SwingUtilities.isEventDispatchThread()) {
            action()
        } else {
            ApplicationManager.getApplication().invokeLater(Runnable(action))
        }
    }

    private fun attach(editor: Editor) {
        val owner = editor.project
        if (owner != null && owner != project) return
        val editorEx = editor as? EditorEx ?: return
        editorEx.addFocusListener(focusListener, this)
    }

    private fun handleEditorActivated(editor: Editor) {
        if (project.isDisposed) return
        if (editor.project != project) return
        val file = FileDocumentManager.getInstance().getFile(editor.document) ?: return
        tabSync.onEditorFocusGained(project, file)
    }

    override fun dispose() {
        if (!disposed.compareAndSet(false, true)) return
        settledTreeFocusProbe.close()
        editorTreeAttachment.close()
        readinessConnection.getAndSet(null)?.let(Disposer::dispose)
        if (!listenerInstalled.compareAndSet(true, false)) {
            editorsRoot.set(null)
            return
        }
        val detach = Runnable {
            try {
                Toolkit.getDefaultToolkit().removeAWTEventListener(editorTreeMouseListener)
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

    companion object {
        private val LOG = Logger.getInstance(EditorFocusSyncListener::class.java)
        private val instances = ConcurrentHashMap<Project, EditorFocusSyncListener>()

        fun install(project: Project, tabSync: EditorTabSyncListener) {
            if (instances.containsKey(project)) return
            val listener =
                startIsolated(
                    create = { EditorFocusSyncListener(project, tabSync) },
                    start = { it.start() },
                    discard = { Disposer.dispose(it) },
                    report = { failure ->
                        LOG.error(
                            "editor focus sync did not start for ${project.name}; project startup continues",
                            failure,
                        )
                    },
                ) ?: return
            if (instances.putIfAbsent(project, listener) != null) Disposer.dispose(listener)
        }

        /**
         * Route an explicit document-scoped focus request through the same command-plane lane as
         * an editor activation. Notification actions know the owning document even when a generic
         * IDE terminal tab is currently showing a different tmux pane (`#jbfocusdocroute`).
         */
        fun routeDocumentFocus(project: Project, documentPath: String): Boolean {
            if (project.isDisposed) return false
            val file = LocalFileSystem.getInstance().findFileByPath(documentPath) ?: return false
            EditorTabSyncListener.install(project).onEditorFocusGained(project, file)
            return true
        }

        fun disposeProject(project: Project) {
            instances.remove(project)?.let { Disposer.dispose(it) }
        }
    }
}
