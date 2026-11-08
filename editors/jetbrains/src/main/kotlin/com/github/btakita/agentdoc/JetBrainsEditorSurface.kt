package com.github.btakita.agentdoc

import com.intellij.openapi.fileEditor.impl.EditorWindow
import com.intellij.openapi.fileEditor.ex.FileEditorManagerEx
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.project.Project
import com.intellij.openapi.wm.ToolWindowManager
import java.awt.KeyboardFocusManager
import java.awt.Component
import java.awt.Window
import javax.swing.SwingUtilities

internal data class JetBrainsEditorSurface(
    val surfaceId: String,
    val toolWindowPaneId: String?,
    val frame: Window?,
    val active: Boolean,
)

/**
 * Resolves JetBrains editor frames to the stable ToolWindowPane identity that
 * `DockWindow` persists for “Show Tab in New Window”. Reflection is localized
 * here because the pane enumeration/frame accessors are platform-internal in
 * build 242 even though the underlying classes and methods are public bytecode.
 */
internal object JetBrainsEditorSurfaces {
    private val LOG = Logger.getInstance(JetBrainsEditorSurfaces::class.java)
    fun forEditorWindow(project: Project, editorWindow: EditorWindow?): JetBrainsEditorSurface {
        val frame = editorWindow?.let(::frameOf)
        val activeFrame = keyboardFocusWindow()
        val pane = toolWindowPanes(project).firstOrNull { pane ->
            paneFrame(pane)?.let { paneFrame -> paneFrame === frame } == true
        }
        val paneId = pane?.let(MarketplacePlatformBridge::paneId)
        return JetBrainsEditorSurface(
            surfaceId = paneId ?: fallbackSurfaceId(frame),
            toolWindowPaneId = paneId,
            frame = frame,
            active = frame != null && windowBelongsToFrame(activeFrame, frame),
        )
    }

    fun activeProjectSurface(project: Project): JetBrainsEditorSurface? {
        val activeFrame = keyboardFocusWindow() ?: return null
        val pane = toolWindowPanes(project).firstOrNull { pane ->
            paneFrame(pane)?.let { frame -> windowBelongsToFrame(activeFrame, frame) } == true
        } ?: return null
        val frame = paneFrame(pane) ?: return null
        val paneId = MarketplacePlatformBridge.paneId(pane) ?: return null
        return JetBrainsEditorSurface(
            surfaceId = paneId,
            toolWindowPaneId = paneId,
            frame = frame,
            active = true,
        )
    }

    /**
     * Focus state lives on the EDT AppContext. Auto-sync rechecks the gate from
     * the focus executor; reading `KeyboardFocusManager` on that thread reports
     * a null active window and drops the select-pane path.
     */
    internal fun keyboardFocusWindow(): Window? {
        val read = { KeyboardFocusManager.getCurrentKeyboardFocusManager().activeWindow }
        if (SwingUtilities.isEventDispatchThread()) {
            return read()
        }
        val holder = arrayOfNulls<Window>(1)
        try {
            SwingUtilities.invokeAndWait { holder[0] = read() }
        } catch (_: InterruptedException) {
            Thread.currentThread().interrupt()
            return null
        } catch (_: Exception) {
            return null
        }
        return holder[0]
    }

    /** The IDE frame is the keyboard surface when focus is that frame or an owned child. */
    internal fun windowBelongsToFrame(active: Window?, frame: Window): Boolean {
        var cursor = active
        var guard = 0
        while (cursor != null && guard < 16) {
            if (cursor === frame) return true
            cursor = cursor.owner
            guard += 1
        }
        return false
    }

    fun isProjectSurfaceActive(project: Project): Boolean = activeProjectSurface(project) != null

    /**
     * Returns authoritative live-surface evidence, or null when the backend exposes no windows.
     *
     * Remote Dev guests can own visible editor windows entirely on the frontend. In that state an
     * empty backend window list is absence of evidence, not evidence that every retained endpoint
     * closed.
     */
    fun liveSurfaceIds(
        project: Project,
        publishedSurfaceId: String? = null,
    ): Set<String>? {
        val manager = FileEditorManagerEx.getInstanceEx(project)
        val enumeratedSurfaceIds =
            manager.windows
                .takeIf { it.isNotEmpty() }
                ?.map { forEditorWindow(project, it).surfaceId }
                ?.toSet()
        val currentSurfaceId =
            manager.currentWindow?.let { forEditorWindow(project, it).surfaceId }
        return authoritativeLiveSurfaceIds(
            enumeratedSurfaceIds,
            currentSurfaceId,
            publishedSurfaceId,
        )
    }

    /**
     * A current/published surface is authoritative positive liveness evidence even when IDEA's
     * `windows` snapshot has not caught up with a detached frame. An absent backend snapshot stays
     * unknown so Remote Dev frontend-owned surfaces are never retired from negative evidence.
     */
    internal fun authoritativeLiveSurfaceIds(
        enumeratedSurfaceIds: Set<String>?,
        vararg currentSurfaceIds: String?,
    ): Set<String>? {
        val live = enumeratedSurfaceIds?.toMutableSet() ?: return null
        currentSurfaceIds
            .filterNotNull()
            .filter(String::isNotBlank)
            .forEach(live::add)
        return live
    }

    fun belongsToAnyEditorSurface(project: Project, component: Component?): Boolean {
        if (component == null) return false
        return FileEditorManagerEx.getInstanceEx(project).windows.any { window ->
            val root = window.tabbedPane.component
            component === root || SwingUtilities.isDescendingFrom(component, root)
        }
    }

    private fun frameOf(editorWindow: EditorWindow): Window? =
        try {
            SwingUtilities.getWindowAncestor(editorWindow.tabbedPane.component)
        } catch (error: Throwable) {
            LOG.debug("[surface] editor frame lookup unavailable", error)
            null
        }

    private fun fallbackSurfaceId(frame: Window?): String =
        if (frame == null) {
            "surface-unavailable"
        } else {
            "frame-${System.identityHashCode(frame).toUInt().toString(16)}"
        }

    private fun toolWindowPanes(project: Project): List<Any> =
        MarketplacePlatformBridge.toolWindowPanes(project)

    private fun paneFrame(pane: Any): Window? = MarketplacePlatformBridge.paneFrame(pane)
}

internal enum class TerminalSurfaceDecisionKind {
    MOUNT,
    STASH,
    ALREADY_MOUNTED,
    EXCLUDE,
    STALE,
    IDLE,
}

internal data class TerminalSurfaceDecision(
    val kind: TerminalSurfaceDecisionKind,
    val surfaceId: String? = null,
    val document: String? = null,
)

internal fun parseTerminalSurfaceDecision(receiptJson: String): TerminalSurfaceDecision? =
    try {
        val decision = com.google.gson.JsonParser.parseString(receiptJson)
            .asJsonObject
            .getAsJsonObject("terminal_decision")
            ?: return null
        val kind = when (decision.get("kind")?.asString) {
            "mount" -> TerminalSurfaceDecisionKind.MOUNT
            "stash" -> TerminalSurfaceDecisionKind.STASH
            "already_mounted" -> TerminalSurfaceDecisionKind.ALREADY_MOUNTED
            "exclude" -> TerminalSurfaceDecisionKind.EXCLUDE
            "stale" -> TerminalSurfaceDecisionKind.STALE
            "idle" -> TerminalSurfaceDecisionKind.IDLE
            else -> return null
        }
        TerminalSurfaceDecision(
            kind = kind,
            surfaceId = decision.get("surface_id")?.asString,
            document = decision.get("document")?.asString,
        )
    } catch (_: Throwable) {
        null
    }
