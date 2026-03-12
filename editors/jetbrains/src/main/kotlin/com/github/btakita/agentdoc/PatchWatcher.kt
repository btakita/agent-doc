package com.github.btakita.agentdoc

import com.intellij.openapi.Disposable
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.LocalFileSystem
import java.io.File
import java.nio.file.FileSystems
import java.nio.file.Path
import java.nio.file.StandardWatchEventKinds
import java.nio.file.WatchService

/**
 * Watches `.agent-doc/patches/` for JSON patch files and applies them
 * via IntelliJ's Document API. This avoids external file change dialogs
 * and cursor jumps that occur when agent-doc writes directly to disk.
 *
 * Flow:
 * 1. `agent-doc write --ipc` writes `<hash>.json` to `.agent-doc/patches/`
 * 2. This watcher detects the new file via NIO WatchService
 * 3. Reads the JSON, finds the target document, applies patches
 * 4. Saves the document and deletes the JSON file (ACK)
 * 5. agent-doc polls for deletion and updates the snapshot
 */
class PatchWatcher(private val project: Project) : Disposable {

    private var watchThread: Thread? = null
    @Volatile private var running = false

    fun start() {
        val basePath = project.basePath ?: return
        val patchesDir = File(basePath, ".agent-doc/patches")
        if (!patchesDir.exists()) {
            patchesDir.mkdirs()
        }

        if (running) return
        running = true

        watchThread = Thread({
            try {
                watchLoop(patchesDir.toPath())
            } catch (e: InterruptedException) {
                // Normal shutdown
            } catch (e: Exception) {
                if (running) {
                    LOG.warn("PatchWatcher error", e)
                }
            }
        }, "agent-doc-patch-watcher").apply {
            isDaemon = true
            start()
        }

        // Process any existing patch files on startup
        processPendingPatches(patchesDir)
    }

    private fun watchLoop(dir: Path) {
        val watchService: WatchService = FileSystems.getDefault().newWatchService()
        dir.register(watchService, StandardWatchEventKinds.ENTRY_CREATE)

        while (running) {
            val key = watchService.poll(500, java.util.concurrent.TimeUnit.MILLISECONDS) ?: continue
            for (event in key.pollEvents()) {
                val filename = event.context() as? Path ?: continue
                if (filename.toString().endsWith(".json")) {
                    val patchFile = dir.resolve(filename).toFile()
                    if (patchFile.exists()) {
                        processPatchFile(patchFile)
                    }
                }
            }
            if (!key.reset()) break
        }

        watchService.close()
    }

    private fun processPendingPatches(dir: File) {
        val files = dir.listFiles { f -> f.extension == "json" } ?: return
        for (file in files) {
            processPatchFile(file)
        }
    }

    private fun processPatchFile(patchFile: File) {
        try {
            val json = patchFile.readText()
            val patch = parsePatchJson(json) ?: return

            ApplicationManager.getApplication().invokeLater {
                try {
                    applyPatch(patch)
                    // ACK: delete the patch file
                    patchFile.delete()
                } catch (e: Exception) {
                    LOG.warn("Failed to apply patch from ${patchFile.name}", e)
                }
            }
        } catch (e: Exception) {
            LOG.warn("Failed to read patch file ${patchFile.name}", e)
        }
    }

    private fun applyPatch(patch: IpcPatch) {
        val targetFile = LocalFileSystem.getInstance().findFileByPath(patch.file) ?: run {
            LOG.warn("Target file not found: ${patch.file}")
            return
        }

        // Refresh to ensure we have latest content
        targetFile.refresh(false, false)

        val document = FileDocumentManager.getInstance().getDocument(targetFile) ?: run {
            LOG.warn("Could not get document for: ${patch.file}")
            return
        }

        WriteCommandAction.runWriteCommandAction(project, "Agent Doc Patch", null, {
            val content = document.text
            var result = content

            for (p in patch.patches) {
                result = applyComponentPatch(result, p.component, p.content)
            }

            // Apply unmatched content to exchange or output component
            if (patch.unmatched.isNotBlank()) {
                result = applyComponentPatch(result, "exchange", patch.unmatched)
                    ?: applyComponentPatch(result, "output", patch.unmatched)
                    ?: result
            }

            if (result != content) {
                document.setText(result)
            }
        })

        // Save the document to disk (so snapshot can read it)
        FileDocumentManager.getInstance().saveDocument(document)
    }

    /**
     * Replace content between `<!-- agent:name -->` and `<!-- /agent:name -->` markers.
     */
    private fun applyComponentPatch(doc: String, component: String, content: String): String {
        val openTag = "<!-- agent:$component -->"
        val closeTag = "<!-- /agent:$component -->"

        val openIdx = doc.indexOf(openTag)
        if (openIdx < 0) return doc

        val contentStart = openIdx + openTag.length
        val closeIdx = doc.indexOf(closeTag, contentStart)
        if (closeIdx < 0) return doc

        val before = doc.substring(0, contentStart)
        val after = doc.substring(closeIdx)

        return before + "\n" + content.trimEnd() + "\n" + after
    }

    override fun dispose() {
        running = false
        watchThread?.interrupt()
        watchThread = null
    }

    companion object {
        private val LOG = com.intellij.openapi.diagnostic.Logger.getInstance(PatchWatcher::class.java)
        private val instances = mutableMapOf<Project, PatchWatcher>()

        fun getInstance(project: Project): PatchWatcher {
            return instances.getOrPut(project) {
                PatchWatcher(project).also { it.start() }
            }
        }

        fun disposeProject(project: Project) {
            instances.remove(project)?.dispose()
        }
    }
}

/** Parsed IPC patch payload. */
data class IpcPatch(
    val file: String,
    val patches: List<ComponentPatch>,
    val unmatched: String,
)

data class ComponentPatch(
    val component: String,
    val content: String,
)

/**
 * Hand-written JSON parser for IPC patch files.
 * Avoids adding a JSON library dependency to the plugin.
 */
fun parsePatchJson(json: String): IpcPatch? {
    try {
        val file = extractStringField(json, "file") ?: return null
        val unmatched = extractStringField(json, "unmatched") ?: ""

        // Parse patches array
        val patchesStart = json.indexOf("\"patches\"")
        if (patchesStart < 0) return null
        val arrayStart = json.indexOf('[', patchesStart)
        if (arrayStart < 0) return null
        val arrayEnd = findMatchingBracket(json, arrayStart) ?: return null
        val patchesJson = json.substring(arrayStart + 1, arrayEnd)

        val patches = mutableListOf<ComponentPatch>()
        var pos = 0
        while (pos < patchesJson.length) {
            val objStart = patchesJson.indexOf('{', pos)
            if (objStart < 0) break
            val objEnd = findMatchingBrace(patchesJson, objStart) ?: break
            val objJson = patchesJson.substring(objStart, objEnd + 1)

            val component = extractStringField(objJson, "component")
            val content = extractStringField(objJson, "content")
            if (component != null && content != null) {
                patches.add(ComponentPatch(component, content))
            }
            pos = objEnd + 1
        }

        return IpcPatch(file, patches, unmatched)
    } catch (e: Exception) {
        return null
    }
}

private fun extractStringField(json: String, field: String): String? {
    val key = "\"$field\""
    val keyIdx = json.indexOf(key)
    if (keyIdx < 0) return null
    val colonIdx = json.indexOf(':', keyIdx + key.length)
    if (colonIdx < 0) return null
    val valueStart = json.indexOf('"', colonIdx + 1)
    if (valueStart < 0) return null
    val valueEnd = findUnescapedQuote(json, valueStart + 1) ?: return null
    return json.substring(valueStart + 1, valueEnd)
        .replace("\\n", "\n")
        .replace("\\t", "\t")
        .replace("\\\"", "\"")
        .replace("\\\\", "\\")
}

private fun findUnescapedQuote(s: String, start: Int): Int? {
    var i = start
    while (i < s.length) {
        if (s[i] == '"' && (i == 0 || s[i - 1] != '\\')) return i
        i++
    }
    return null
}

private fun findMatchingBracket(s: String, start: Int): Int? {
    var depth = 0
    var inString = false
    var i = start
    while (i < s.length) {
        val c = s[i]
        if (c == '"' && (i == 0 || s[i - 1] != '\\')) inString = !inString
        if (!inString) {
            if (c == '[') depth++
            if (c == ']') { depth--; if (depth == 0) return i }
        }
        i++
    }
    return null
}

private fun findMatchingBrace(s: String, start: Int): Int? {
    var depth = 0
    var inString = false
    var i = start
    while (i < s.length) {
        val c = s[i]
        if (c == '"' && (i == 0 || s[i - 1] != '\\')) inString = !inString
        if (!inString) {
            if (c == '{') depth++
            if (c == '}') { depth--; if (depth == 0) return i }
        }
        i++
    }
    return null
}
