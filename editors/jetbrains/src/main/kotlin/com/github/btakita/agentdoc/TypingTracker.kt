package com.github.btakita.agentdoc

import com.intellij.openapi.editor.event.DocumentEvent
import com.intellij.openapi.editor.event.DocumentListener
import com.intellij.openapi.fileEditor.FileDocumentManager
import java.util.UUID

object EditorIdentity {
    val id: String = "jetbrains-${ProcessHandle.current().pid()}-${UUID.randomUUID()}"
}

/**
 * Tracks document changes and provides debounce via the FFI shared library.
 *
 * On every .md document change, forwards the event to `agent_doc_document_changed()`.
 * Before submission, `awaitIdle()` blocks until the user stops typing.
 *
 * Registered as a bulk DocumentListener in PluginLifecycleListener.
 */
object TypingTracker : DocumentListener {

    const val DEBOUNCE_MS = 500L
    private const val TIMEOUT_MS = 30000L
    private val LOG = com.intellij.openapi.diagnostic.Logger.getInstance(TypingTracker::class.java)

    override fun documentChanged(event: DocumentEvent) {
        val vFile = FileDocumentManager.getInstance().getFile(event.document) ?: return
        if (!vFile.name.endsWith(".md")) return

        // Forward to FFI debounce tracker
        val lib = AgentDocLib.get()
        if (lib != null) {
            val text = event.document.text
            // #pcp6: send the full editor buffer content for .md session documents
            // so the CLI visible-write reconcile guard can positively confirm the
            // editor buffer equals on-disk content (no unsaved edit ahead of disk)
            // instead of inferring from a len/hash digest. The text stays local to
            // the project .agent-doc/ state dir.
            try {
                lib.agent_doc_document_changed_digest_content_for_editor(
                    vFile.path,
                    text,
                    EditorIdentity.id,
                )
            } catch (_: UnsatisfiedLinkError) {
                lib.agent_doc_document_changed_digest_content(vFile.path, text)
            } catch (_: NoSuchMethodError) {
                lib.agent_doc_document_changed_digest_content(vFile.path, text)
            }
            LOG.debug("[native] document_changed (content): ${vFile.name}")
        } else {
            // Fallback: track locally if FFI unavailable
            lastChangeMs = System.currentTimeMillis()
            LOG.debug("[fallback] document_changed: ${vFile.name}")
        }
    }

    /** Block until the document has been idle, or timeout. Returns true if idle. */
    fun awaitIdle(filePath: String): Boolean {
        val lib = AgentDocLib.get()
        return if (lib != null) {
            LOG.info("[native] awaitIdle: waiting for idle (${DEBOUNCE_MS}ms debounce, ${TIMEOUT_MS}ms timeout)")
            val result = lib.agent_doc_await_idle(filePath, DEBOUNCE_MS, TIMEOUT_MS)
            LOG.info("[native] awaitIdle: result=$result")
            result
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
            // Check if file is tracked — if not, fall back to local tracking
            // (untracked files return idle=true from await_idle, which is misleading)
            if (!lib.agent_doc_is_tracked(filePath)) {
                val local = (System.currentTimeMillis() - lastChangeMs) < DEBOUNCE_MS
                LOG.info("[native] isRecentlyTyping: file untracked, fallback local=$local")
                return local
            }
            // await_idle with 0 timeout = non-blocking check
            val idle = lib.agent_doc_await_idle(filePath, DEBOUNCE_MS, 0)
            LOG.info("[native] isRecentlyTyping: idle=$idle → recentlyTyping=${!idle}")
            !idle
        } else {
            val result = (System.currentTimeMillis() - lastChangeMs) < DEBOUNCE_MS
            LOG.info("[fallback] isRecentlyTyping: result=$result")
            result
        }
    }

    // Fallback local tracking (used when FFI unavailable or file untracked)
    @Volatile
    private var lastChangeMs: Long = 0
}
