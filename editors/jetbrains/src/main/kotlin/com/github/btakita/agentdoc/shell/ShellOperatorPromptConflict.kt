package com.github.btakita.agentdoc.shell

import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.editor.Document
import com.intellij.openapi.fileEditor.FileDocumentManagerListener
import com.intellij.openapi.fileEditor.FileDocumentSynchronizationVetoer
import com.intellij.openapi.vfs.VirtualFile

/*
 * Idle save refreshes the virtual file before the vetoer runs, and that refresh
 * queues the File Cache Conflict dialog. The save listener keeps editor memory
 * for that one save batch. The vetoer still refuses a later Load FS / reload
 * that would drop an unmarked operator prompt or replace it with a seen row.
 */

private fun operatorPromptExtensions(): ImplEditorExtensions? =
    ShellGenerationHost.getInstance().editorExtensions()

/** `fileDocumentSynchronizationVetoer`: refuse a reload that drops an unmarked prompt. */
class ShellOperatorPromptReloadVetoer : FileDocumentSynchronizationVetoer() {
    override fun mayReloadFileContent(file: VirtualFile, document: Document): Boolean {
        val drops = operatorPromptExtensions()?.reloadDropsUnmarkedOperatorPrompt(
            document.charsSequence.toString(),
            diskText(file),
        ) ?: return true
        if (!drops) return true
        LOG.info(
            "[agent-doc] File Cache Conflict reload refused for ${file.path}; " +
                "it would drop an unmarked operator prompt",
        )
        return false
    }

    private fun diskText(file: VirtualFile): String? =
        try {
            String(file.contentsToByteArray(), file.charset)
        } catch (_: Exception) {
            null
        }

    private companion object {
        val LOG: Logger = Logger.getInstance(ShellOperatorPromptReloadVetoer::class.java)
    }
}

/** `fileDocumentManagerListener`: keep memory across the idle-save refresh. */
class ShellOperatorPromptSaveListener : FileDocumentManagerListener {
    override fun beforeAllDocumentsSaving() {
        operatorPromptExtensions()?.holdAgentDocMemoryAcrossSaveBatch()
    }
}
