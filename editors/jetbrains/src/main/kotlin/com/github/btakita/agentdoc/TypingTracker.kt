package com.github.btakita.agentdoc

import com.intellij.openapi.editor.event.DocumentEvent
import com.intellij.openapi.editor.event.DocumentListener
import com.intellij.openapi.fileEditor.FileDocumentManager

/**
 * Tracks document changes and provides debounce via the FFI shared library.
 *
 * On every .md document change, forwards the event to `agent_doc_document_changed()`.
 * Before submission, `awaitIdle()` blocks until the user stops typing.
 *
 * Registered as a bulk DocumentListener in PluginLifecycleListener.
 */
object TypingTracker : DocumentListener {

    const val DEBOUNCE_MS = 1500L
    private const val TIMEOUT_MS = 30000L

    override fun documentChanged(event: DocumentEvent) {
        val vFile = FileDocumentManager.getInstance().getFile(event.document) ?: return
        if (!vFile.name.endsWith(".md")) return

        // Forward to FFI debounce tracker
        val lib = AgentDocLib.get()
        if (lib != null) {
            lib.agent_doc_document_changed(vFile.path)
        } else {
            // Fallback: track locally if FFI unavailable
            lastChangeMs = System.currentTimeMillis()
        }
    }

    /** Block until the document has been idle, or timeout. Returns true if idle. */
    fun awaitIdle(filePath: String): Boolean {
        val lib = AgentDocLib.get()
        return if (lib != null) {
            lib.agent_doc_await_idle(filePath, DEBOUNCE_MS, TIMEOUT_MS)
        } else {
            // Fallback: simple local check
            val elapsed = System.currentTimeMillis() - lastChangeMs
            if (elapsed >= DEBOUNCE_MS) return true
            Thread.sleep(DEBOUNCE_MS - elapsed)
            true
        }
    }

    /** Check if the user was recently typing (for conditional debounce). */
    fun isRecentlyTyping(filePath: String): Boolean {
        val lib = AgentDocLib.get()
        return if (lib != null) {
            // await_idle with 0 timeout = non-blocking check
            !lib.agent_doc_await_idle(filePath, DEBOUNCE_MS, 0)
        } else {
            (System.currentTimeMillis() - lastChangeMs) < DEBOUNCE_MS
        }
    }

    // Fallback local tracking (used when FFI unavailable)
    @Volatile
    private var lastChangeMs: Long = 0
}
