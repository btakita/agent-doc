@file:Suppress("UnstableApiUsage", "DEPRECATION", "removal")

package com.github.btakita.agentdoc.split.frontend

import com.github.btakita.agentdoc.split.FrontendPresentationCapability
import com.github.btakita.agentdoc.split.EditorWindowSnapshot
import com.github.btakita.agentdoc.split.FrontendSurface
import com.github.btakita.agentdoc.split.FrontendSurfaceSnapshot
import com.github.btakita.agentdoc.split.FrontendPresentationKind
import com.github.btakita.agentdoc.split.FrontendPresentationProjection
import com.github.btakita.agentdoc.split.FrontendPresentationReceipt
import com.github.btakita.agentdoc.split.FrontendPresentationReceiptOutcome
import com.github.btakita.agentdoc.split.FrontendSurfacePresentation
import com.github.btakita.agentdoc.split.SurfaceRole
import com.github.btakita.agentdoc.split.MAIN_SURFACE_PANE_ID
import com.intellij.openapi.Disposable
import com.intellij.openapi.application.ApplicationInfo
import com.intellij.openapi.fileEditor.ex.FileEditorManagerEx
import com.intellij.openapi.fileEditor.impl.EditorWindow
import com.intellij.openapi.fileEditor.impl.FileEditorOpenOptions
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.Disposer
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.openapi.wm.WindowManager
import com.intellij.platform.project.projectId
import com.intellij.terminal.frontend.toolwindow.TerminalToolWindowTab
import com.intellij.terminal.frontend.toolwindow.TerminalToolWindowTabsManager
import java.util.Collections
import java.util.WeakHashMap
import java.awt.Component
import java.awt.KeyboardFocusManager
import java.awt.Point
import java.awt.Window
import javax.swing.SwingUtilities

internal object FrontendSurfaceCollector {
    fun capture(
        project: Project,
        frontendInstanceId: String,
        sequence: Long,
        surfaceIdentities: FrontendSurfaceIdentityTracker,
    ): FrontendSurfaceSnapshot = captureWithTargets(project, frontendInstanceId, sequence, surfaceIdentities).snapshot

    fun captureWithTargets(
        project: Project,
        frontendInstanceId: String,
        sequence: Long,
        surfaceIdentities: FrontendSurfaceIdentityTracker,
    ): CapturedFrontendSurfaces {
        check(SwingUtilities.isEventDispatchThread()) { "frontend surface capture must run on EDT" }
        val activeFrame = KeyboardFocusManager.getCurrentKeyboardFocusManager().activeWindow
        val mainFrame = WindowManager.getInstance().getFrame(project)
        val editorWindows = FileEditorManagerEx.getInstanceEx(project).windows.toList()
        val tagged = editorWindows.mapNotNull { editorWindow ->
            val component = editorWindow.tabbedPane.component
            val frame = SwingUtilities.getWindowAncestor(component) ?: return@mapNotNull null
            TaggedEditorWindow(frame, componentPosition(component), editorWindow)
        }
        val complete = mainFrame != null && tagged.size == editorWindows.size
        val grouped = tagged.groupBy(TaggedEditorWindow::frame).toMutableMap()
        if (mainFrame != null) grouped.putIfAbsent(mainFrame, emptyList())
        val assignments = surfaceIdentities.assign(grouped.keys, mainFrame, complete)
        val ordered = grouped.map { (frame, surfaceWindows) ->
            assignments.getValue(frame) to (frame to surfaceWindows.sortedWith(compareBy({ it.position.x }, { it.position.y })))
        }
        val surfaces = ordered.map { (identity, frameAndWindows) ->
            val (frame, windows) = frameAndWindows
            FrontendSurface(
                identity.paneId,
                identity.surfaceGeneration,
                identity.role,
                frame === activeFrame,
                windows.mapIndexed { ordinal, taggedWindow -> editorWindowSnapshot(taggedWindow.editorWindow).copy(ordinal = ordinal) },
            )
        }.sortedWith(compareBy({ it.role != SurfaceRole.MAIN }, FrontendSurface::paneId))
        val targets = ordered.associate { (identity, frameAndWindows) ->
            identity.paneId to FrontendSurfaceTarget(
                identity.paneId,
                identity.surfaceGeneration,
                identity.role,
                frameAndWindows.second.map { taggedWindow ->
                    FrontendEditorWindowTarget(
                        taggedWindow.editorWindow,
                        taggedWindow.editorWindow.fileList.map(SurfacePresentationFiles::logicalPath).toSet(),
                    )
                },
            )
        }
        return CapturedFrontendSurfaces(
            FrontendSurfaceSnapshot(
                frontendInstanceId = frontendInstanceId,
                sequence = sequence,
                projectId = project.projectId(),
                complete = complete,
                presentationCapability = FrontendPresentationCapability.SNAPSHOT_ONLY,
                surfaces = surfaces,
            ),
            targets,
        )
    }

    internal fun roleForPaneId(paneId: String): SurfaceRole =
        if (paneId == MAIN_SURFACE_PANE_ID) SurfaceRole.MAIN else SurfaceRole.DETACHED

    private fun editorWindowSnapshot(window: EditorWindow): EditorWindowSnapshot {
        val openPaths = window.fileList.map(SurfacePresentationFiles::logicalPath).distinct()
        val selectedPath = window.selectedFile?.let(SurfacePresentationFiles::logicalPath)
        return EditorWindowSnapshot(0, selectedPath, openPaths, listOfNotNull(selectedPath))
    }

    private fun componentPosition(component: Component): Point =
        try {
            component.locationOnScreen
        } catch (_: IllegalStateException) {
            component.location
        }

    private data class TaggedEditorWindow(val frame: Window, val position: Point, val editorWindow: EditorWindow)
}

internal data class CapturedFrontendSurfaces(
    val snapshot: FrontendSurfaceSnapshot,
    val targets: Map<String, FrontendSurfaceTarget>,
)

internal data class FrontendSurfaceTarget(
    val paneId: String,
    val surfaceGeneration: Long,
    val role: SurfaceRole,
    val windows: List<FrontendEditorWindowTarget>,
)

internal data class FrontendEditorWindowTarget(val editorWindow: EditorWindow, val logicalPaths: Set<String>)

/**
 * The exact-build internal adapter is the only place allowed to import 262 editor-window APIs.
 * The artifact is clamped to 262.* and the shape is probed before terminal capability is admitted.
 */
internal class Exact262DetachedPresentationAdapter private constructor(
    private val project: Project,
    parentDisposable: Disposable,
) : Disposable {
    val capability: FrontendPresentationCapability = FrontendPresentationCapability.EXACT_262_INTERNAL
    private val manager = FileEditorManagerEx.getInstanceEx(project)
    private val terminalTabs = TerminalToolWindowTabsManager.getInstance(project)
    private val mounted = mutableMapOf<SurfaceIncarnation, MountedPresentation>()

    init {
        Disposer.register(parentDisposable, this)
    }

    fun apply(
        capture: CapturedFrontendSurfaces,
        projection: FrontendPresentationProjection,
    ): List<FrontendPresentationReceipt> {
        check(SwingUtilities.isEventDispatchThread()) { "detached presentation must run on EDT" }
        if (projection.revision != capture.snapshot.sequence) {
            return projection.presentations.map { it.receipt(FrontendPresentationReceiptOutcome.STALE, "capture revision changed") }
        }
        val live = capture.targets.values.map { it.incarnation() }.toSet()
        // A rejoin can move the same EditorWindow back under the main frame. Restore the
        // original document before removing our bookkeeping so a terminal cannot leak to main.
        mountedSurfaceKeysToRestore(mounted.keys, live).forEach(::restore)
        return projection.presentations.map { presentation -> applyOne(capture, presentation) }
    }

    private fun applyOne(
        capture: CapturedFrontendSurfaces,
        presentation: FrontendSurfacePresentation,
    ): FrontendPresentationReceipt {
        val target = capture.targets[presentation.identity.surfaceId]
        if (target == null ||
            target.surfaceGeneration != presentation.identity.surfaceGeneration ||
            target.role != SurfaceRole.DETACHED
        ) {
            return presentation.receipt(FrontendPresentationReceiptOutcome.STALE, "exact detached target is unavailable")
        }
        val key = target.incarnation()
        val current = mounted[key]
        if (current != null && current.revision > presentation.presentationRevision) {
            return presentation.receipt(FrontendPresentationReceiptOutcome.STALE, "newer presentation is already mounted")
        }
        if (current?.matches(presentation) == true) {
            return presentation.receipt(FrontendPresentationReceiptOutcome.APPLIED)
        }
        return try {
            when (presentation.kind) {
                FrontendPresentationKind.EMPTY -> restore(key)
                FrontendPresentationKind.PLACEHOLDER -> mountPlaceholder(exactWindow(target, presentation), key, presentation)
                FrontendPresentationKind.TERMINAL -> mountTerminal(exactWindow(target, presentation), key, presentation)
            }
            presentation.receipt(FrontendPresentationReceiptOutcome.APPLIED)
        } catch (failure: Throwable) {
            presentation.receipt(
                FrontendPresentationReceiptOutcome.REFUSED,
                failure.message ?: failure.javaClass.simpleName,
            )
        }
    }

    private fun exactWindow(
        target: FrontendSurfaceTarget,
        presentation: FrontendSurfacePresentation,
    ): EditorWindow {
        val document = requireNotNull(presentation.document) { "presentation missing document" }
        val index = exactWindowIndex(target.windows.map(FrontendEditorWindowTarget::logicalPaths), document)
        requireNotNull(index) { "presentation document must identify exactly one editor split" }
        return target.windows[index].editorWindow
    }

    private fun mountPlaceholder(
        window: EditorWindow,
        key: SurfaceIncarnation,
        presentation: FrontendSurfacePresentation,
    ) {
        val document = requireNotNull(presentation.document) { "placeholder presentation missing document" }
        val original = requireNotNull(LocalFileSystem.getInstance().findFileByPath(document)) {
            "placeholder document is unavailable"
        }
        val replacement = DetachedPlaceholderVirtualFile(
            logicalDocument = document,
            message = placeholderMessage(presentation.reason),
        )
        replace(window, key, presentation, original, replacement, null)
    }

    private fun mountTerminal(
        window: EditorWindow,
        key: SurfaceIncarnation,
        presentation: FrontendSurfacePresentation,
    ) {
        val document = requireNotNull(presentation.document) { "terminal presentation missing document" }
        val viewSession = requireNotNull(presentation.viewSession) {
            "terminal presentation missing controller-derived view session"
        }
        val original = requireNotNull(LocalFileSystem.getInstance().findFileByPath(document)) {
            "terminal document is unavailable"
        }
        val tab = terminalTabs.createTabBuilder()
            .workingDirectory(project.basePath)
            .shellCommand(terminalCommand(viewSession))
            .tabName("Agent Doc isolated view")
            .requestFocus(true)
            .deferSessionStartUntilUiShown(true)
            .shouldAddToToolWindow(false)
            .createTab()
        val replacement = terminalVirtualFile(tab)
        SurfacePresentationFiles.register(replacement, document)
        try {
            replace(window, key, presentation, original, replacement, tab)
        } catch (failure: Throwable) {
            SurfacePresentationFiles.unregister(replacement)
            terminalTabs.closeTab(tab)
            throw failure
        }
    }

    private fun terminalVirtualFile(tab: TerminalToolWindowTab): VirtualFile =
        Class.forName("com.intellij.terminal.frontend.editor.TerminalViewVirtualFile")
            .constructors
            .single { constructor -> constructor.parameterCount == 2 }
            .newInstance(tab.view, false) as VirtualFile

    private fun replace(
        window: EditorWindow,
        key: SurfaceIncarnation,
        presentation: FrontendSurfacePresentation,
        original: VirtualFile,
        replacement: VirtualFile,
        terminalTab: TerminalToolWindowTab?,
    ) {
        restore(key)
        manager.openFile(
            replacement,
            window,
            FileEditorOpenOptions().withRequestFocus(true),
        )
        check(window.fileList.any { it === replacement }) { "replacement did not open in exact editor window" }
        manager.closeFile(original, window)
        mounted[key] = MountedPresentation(
            window = window,
            original = original,
            replacement = replacement,
            terminalTab = terminalTab,
            revision = presentation.presentationRevision,
            kind = presentation.kind,
            document = presentation.document,
            viewSession = presentation.viewSession,
        )
    }

    private fun restore(key: SurfaceIncarnation) {
        val prior = mounted.remove(key) ?: return
        if (!prior.window.isDisposed) {
            manager.openFile(
                prior.original,
                prior.window,
                FileEditorOpenOptions().withRequestFocus(false),
            )
            manager.closeFile(prior.replacement, prior.window)
        }
        SurfacePresentationFiles.unregister(prior.replacement)
        prior.terminalTab?.let(terminalTabs::closeTab)
    }

    override fun dispose() {
        check(SwingUtilities.isEventDispatchThread()) { "detached presentation disposal must run on EDT" }
        mounted.keys.toList().forEach(::restore)
    }

    private fun FrontendSurfacePresentation.receipt(
        outcome: FrontendPresentationReceiptOutcome,
        diagnostic: String? = null,
    ) = FrontendPresentationReceipt(
        projectId = project.projectId(),
        identity = identity,
        presentationRevision = presentationRevision,
        kind = kind,
        document = document,
        viewSession = viewSession,
        outcome = outcome,
        diagnostic = diagnostic,
    )

    companion object {
        fun create(project: Project, parentDisposable: Disposable): Exact262DetachedPresentationAdapter? =
            if (runtimeShapeAvailable()) {
                runCatching { Exact262DetachedPresentationAdapter(project, parentDisposable) }.getOrNull()
            } else {
                null
            }

        internal fun runtimeShapeAvailable(): Boolean = runCatching {
            check(ApplicationInfo.getInstance().build.baselineVersion == 262)
            val project = Class.forName("com.intellij.openapi.project.Project")
            val virtualFile = Class.forName("com.intellij.openapi.vfs.VirtualFile")
            val editorWindow = Class.forName("com.intellij.openapi.fileEditor.impl.EditorWindow")
            val options = Class.forName("com.intellij.openapi.fileEditor.impl.FileEditorOpenOptions")
            options.getConstructor()
            options.getMethod("withRequestFocus", Boolean::class.javaPrimitiveType)
            val editorManager = Class.forName("com.intellij.openapi.fileEditor.ex.FileEditorManagerEx")
            editorManager.getMethod("getInstanceEx", project)
            editorManager.getMethod("openFile", virtualFile, editorWindow, options)
            editorManager.getMethod("closeFile", virtualFile, editorWindow)
            val manager = Class.forName("com.intellij.terminal.frontend.toolwindow.TerminalToolWindowTabsManager")
            val tab = Class.forName("com.intellij.terminal.frontend.toolwindow.TerminalToolWindowTab")
            val terminalView = Class.forName("com.intellij.terminal.frontend.view.TerminalView")
            manager.getMethod("getInstance", project)
            manager.getMethod("createTabBuilder")
            manager.getMethod("closeTab", tab)
            val builder = Class.forName("com.intellij.terminal.frontend.toolwindow.TerminalToolWindowTabBuilder")
            builder.getMethod("workingDirectory", String::class.java)
            builder.getMethod("shellCommand", List::class.java)
            builder.getMethod("tabName", String::class.java)
            builder.getMethod("requestFocus", Boolean::class.javaPrimitiveType)
            builder.getMethod("deferSessionStartUntilUiShown", Boolean::class.javaPrimitiveType)
            builder.getMethod("shouldAddToToolWindow", Boolean::class.javaPrimitiveType)
            builder.getMethod("createTab")
            tab.getMethod("getView")
            Class.forName("com.intellij.terminal.frontend.editor.TerminalViewVirtualFile")
                .getConstructor(terminalView, Boolean::class.javaPrimitiveType)
            true
        }.getOrDefault(false)

        internal fun terminalCommand(viewSession: String): List<String> =
            listOf("tmux", "attach-session", "-t", viewSession)

        internal fun placeholderMessage(reason: String?): String = when (reason) {
            "main_owned" -> "This Agent Doc session is interactive in the main IDE window."
            "owned_by_other_detached_surface" -> "This Agent Doc session is interactive in another detached window."
            "binding_pending" -> "Preparing an isolated Agent Doc terminal…"
            "release_pending" -> "Returning this Agent Doc session to the main layout…"
            else -> "This Agent Doc session is not interactive in this window."
        }
    }

    private data class MountedPresentation(
        val window: EditorWindow,
        val original: VirtualFile,
        val replacement: VirtualFile,
        val terminalTab: TerminalToolWindowTab?,
        val revision: Long,
        val kind: FrontendPresentationKind,
        val document: String?,
        val viewSession: String?,
    ) {
        fun matches(presentation: FrontendSurfacePresentation): Boolean =
            kind == presentation.kind &&
                document == presentation.document &&
                viewSession == presentation.viewSession
    }
}

internal data class SurfaceIncarnation(val paneId: String, val surfaceGeneration: Long)

private fun FrontendSurfaceTarget.incarnation() = SurfaceIncarnation(paneId, surfaceGeneration)

internal fun exactWindowIndex(windowPaths: List<Set<String>>, document: String): Int? =
    windowPaths.indices.filter { document in windowPaths[it] }.singleOrNull()

internal fun mountedSurfaceKeysToRestore(
    mounted: Collection<SurfaceIncarnation>,
    live: Set<SurfaceIncarnation>,
): Set<SurfaceIncarnation> = mounted.filterNot(live::contains).toSet()

internal object SurfacePresentationFiles {
    private val logicalDocuments = Collections.synchronizedMap(WeakHashMap<VirtualFile, String>())

    fun register(file: VirtualFile, logicalDocument: String) {
        logicalDocuments[file] = logicalDocument
    }

    fun unregister(file: VirtualFile) {
        logicalDocuments.remove(file)
    }

    fun logicalPath(file: VirtualFile): String =
        (file as? DetachedPlaceholderVirtualFile)?.logicalDocument ?: logicalDocuments[file] ?: file.path
}
