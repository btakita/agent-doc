package com.github.btakita.agentdoc

import com.intellij.openapi.Disposable
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.util.Alarm
import java.io.File

/**
 * `#lzlosstree` Phase 5 plugin apply side: poll the lossless-tree frame the binary
 * drops for a tree-capable session and apply it to the live buffer.
 *
 * The binary owns the frame path ([LosslessTreeFrames.framePath]); when a frame is
 * present the watcher renders it ([LosslessTreeFrames.renderFrame]) and replaces the
 * document text on the EDT inside a single undoable write, then deletes the frame as
 * an ACK. Rendering + hashing live in the native library (FFI-first) — the plugin only
 * moves the resulting text into the IntelliJ [com.intellij.openapi.editor.Document].
 *
 * The watcher is a thin transport consumer; the merge/authority decisions were already
 * made server-side (only tree-capable sessions ever get a frame, so applying one can
 * never regress a non-capable session).
 */
class LosslessTreeFrameWatcher(
    private val project: Project,
    private val filePath: String,
) : Disposable {
    private val alarm = Alarm(Alarm.ThreadToUse.POOLED_THREAD, this)

    /** Begin polling this document's frame path on a background alarm. */
    fun start() {
        schedule()
    }

    private fun schedule() {
        if (alarm.isDisposed) return
        alarm.addRequest({
            try {
                applyPendingFrame(project, filePath)
            } catch (e: Throwable) {
                LOG.debug("[lossless-frame] poll failed for $filePath: ${e.message}")
            } finally {
                schedule()
            }
        }, POLL_INTERVAL_MS)
    }

    override fun dispose() {
        alarm.cancelAllRequests()
    }

    companion object {
        private val LOG = Logger.getInstance(LosslessTreeFrameWatcher::class.java)
        private const val POLL_INTERVAL_MS = 150

        /**
         * If a lossless-tree frame is pending for [filePath], render it and replace the
         * document buffer, then delete the frame as an ACK. Returns true when a frame
         * was applied (or the buffer was already current). A corrupt/unreadable frame is
         * left in place (the binary can re-emit) and the buffer is untouched.
         */
        fun applyPendingFrame(project: Project, filePath: String): Boolean {
            val framePath = LosslessTreeFrames.framePath(filePath) ?: return false
            val frameFile = File(framePath)
            if (!frameFile.exists()) return false
            val rendered = LosslessTreeFrames.renderFrame(framePath) ?: return false

            var applied = false
            ApplicationManager.getApplication().invokeAndWait {
                val vf = LocalFileSystem.getInstance().findFileByPath(filePath) ?: return@invokeAndWait
                val document = FileDocumentManager.getInstance().getDocument(vf) ?: return@invokeAndWait
                if (document.text == rendered) {
                    applied = true
                    return@invokeAndWait
                }
                WriteCommandAction.runWriteCommandAction(project) {
                    document.setText(rendered)
                }
                applied = true
                LOG.info("[lossless-frame] applied frame for $filePath (${rendered.length} chars)")
            }
            // ACK: the frame is consumed once the buffer reflects it.
            if (applied) frameFile.delete()
            return applied
        }
    }
}
