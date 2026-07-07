package com.github.btakita.agentdoc

import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.FileEditorManagerListener
import com.intellij.openapi.vfs.VirtualFile
import java.util.concurrent.ConcurrentHashMap

/**
 * `#lzlosstree` Phase 5: start a [LosslessTreeFrameWatcher] for each opened markdown
 * document so a tree-capable session's frames are applied to the live buffer, and stop
 * it on close. Registered as a project [FileEditorManagerListener] in `plugin.xml`.
 *
 * The watcher poll is a cheap no-op unless the binary actually drops a frame (only
 * tree-capable sessions get one), so watching every `.md` editor is safe; the
 * frame-path/render decisions all live in the native library.
 */
class LosslessTreeFrameListener : FileEditorManagerListener {
    private val watchers = ConcurrentHashMap<String, LosslessTreeFrameWatcher>()

    override fun fileOpened(source: FileEditorManager, file: VirtualFile) {
        if (file.extension != "md") return
        val path = file.path
        watchers.computeIfAbsent(path) {
            LosslessTreeFrameWatcher(source.project, path).also { it.start() }
        }
    }

    override fun fileClosed(source: FileEditorManager, file: VirtualFile) {
        watchers.remove(file.path)?.dispose()
    }
}
