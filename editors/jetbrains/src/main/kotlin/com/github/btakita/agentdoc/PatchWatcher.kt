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
                val applied = try {
                    applyPatch(patch)
                } catch (e: Exception) {
                    LOG.warn("Failed to apply patch from ${patchFile.name}", e)
                    false
                }
                if (applied) {
                    patchFile.delete()
                } else {
                    LOG.warn("Patch not applied, leaving file for retry: ${patchFile.name}")
                }
            }
        } catch (e: Exception) {
            LOG.warn("Failed to read patch file ${patchFile.name}", e)
        }
    }

    private fun applyPatch(patch: IpcPatch): Boolean {
        var targetFile = LocalFileSystem.getInstance().findFileByPath(patch.file)
        if (targetFile == null) {
            // Retry once after a short delay — file might not be indexed yet
            Thread.sleep(200)
            LocalFileSystem.getInstance().refresh(false)
            targetFile = LocalFileSystem.getInstance().findFileByPath(patch.file)
        }
        if (targetFile == null) {
            LOG.warn("Target file not found: ${patch.file}")
            return false
        }

        // Refresh to ensure we have latest content
        targetFile.refresh(false, false)

        val document = FileDocumentManager.getInstance().getDocument(targetFile) ?: run {
            LOG.warn("Could not get document for: ${patch.file}")
            return false
        }

        WriteCommandAction.runWriteCommandAction(project, "Agent Doc Patch", null, {
            val content = document.text

            // Full content replacement (append-mode documents without component markers)
            if (!patch.fullContent.isNullOrEmpty()) {
                if (patch.fullContent != content) {
                    document.setText(patch.fullContent)
                }
                return@runWriteCommandAction
            }

            // Component-based patching (template/stream-mode documents)
            var result = content

            // Apply frontmatter patch first (before component patches)
            if (!patch.frontmatter.isNullOrBlank()) {
                result = NativePatching.mergeFrontmatter(result, patch.frontmatter)
                    ?: applyFrontmatterPatchKotlin(result, patch.frontmatter)
            }

            for (p in patch.patches) {
                result = applyComponentPatchNative(result, p.component, p.content)
            }

            // Apply unmatched content to exchange or output component
            if (patch.unmatched.isNotBlank()) {
                val exchangeResult = applyComponentPatchNative(result, "exchange", patch.unmatched)
                result = if (exchangeResult != result) exchangeResult
                    else applyComponentPatchNative(result, "output", patch.unmatched)
            }

            if (result != content) {
                document.setText(result)
                LOG.info("Patch applied to ${patch.file} (${result.length - content.length} chars changed)")
            } else {
                LOG.warn("Patch produced no changes for ${patch.file}")
            }
        })

        // Save the document to disk (so snapshot can read it)
        FileDocumentManager.getInstance().saveDocument(document)
        return true
    }

    /**
     * Apply a component patch, preferring native FFI with Kotlin fallback.
     *
     * The native library handles code block detection, attribute parsing,
     * and mode resolution identically to the CLI — eliminating duplicated logic.
     */
    private fun applyComponentPatchNative(doc: String, component: String, content: String): String {
        // Native: resolve mode from inline attributes + components.toml + defaults
        // The FFI apply_patch requires an explicit mode, but the Kotlin fallback
        // extracts mode from inline attributes. For the FFI path, we extract the
        // mode from the document first, then call apply_patch.
        val mode = extractComponentMode(doc, component)
        return NativePatching.applyComponentPatch(doc, component, content, mode)
            ?: applyComponentPatchKotlin(doc, component, content)
    }

    /**
     * Extract the patch mode for a component from its inline attributes.
     * Returns "replace" as default if not specified.
     */
    private fun extractComponentMode(doc: String, component: String): String {
        val openPattern = Regex("""<!-- agent:${Regex.escape(component)}(\s[^>]*)? -->""")
        val match = openPattern.find(doc) ?: return "replace"
        val attrs = match.groupValues.getOrNull(1) ?: return "replace"
        val patchMatch = Regex("""patch=(\w+)""").find(attrs)
        val modeMatch = Regex("""mode=(\w+)""").find(attrs)
        return patchMatch?.groupValues?.getOrNull(1)
            ?: modeMatch?.groupValues?.getOrNull(1) ?: "replace"
    }

    /**
     * Kotlin fallback: merge YAML key/value pairs into the document's frontmatter.
     * Parses the existing frontmatter, updates matching keys, preserves others.
     */
    private fun applyFrontmatterPatchKotlin(doc: String, yamlFields: String): String {
        if (!doc.startsWith("---\n")) return doc

        val endIdx = doc.indexOf("\n---\n", 4)
        if (endIdx < 0) return doc

        val existingYaml = doc.substring(4, endIdx)
        val body = doc.substring(endIdx + 5) // skip \n---\n

        // Parse existing frontmatter as key/value pairs (preserve order)
        val existing = LinkedHashMap<String, String>()
        for (line in existingYaml.lines()) {
            val colonIdx = line.indexOf(':')
            if (colonIdx > 0) {
                val key = line.substring(0, colonIdx).trim()
                val value = line.substring(colonIdx + 1).trim()
                existing[key] = value
            }
        }

        // Merge new fields
        for (line in yamlFields.lines()) {
            val colonIdx = line.indexOf(':')
            if (colonIdx > 0) {
                val key = line.substring(0, colonIdx).trim()
                val value = line.substring(colonIdx + 1).trim()
                if (key.isNotEmpty()) {
                    existing[key] = value
                }
            }
        }

        // Rebuild frontmatter
        val newYaml = existing.entries.joinToString("\n") { "${it.key}: ${it.value}" }
        return "---\n$newYaml\n---\n$body"
    }

    /**
     * Kotlin fallback: replace content between component markers.
     * Used when native library is unavailable.
     */
    private fun applyComponentPatchKotlin(doc: String, component: String, content: String): String {
        // Match open tag with optional attributes: <!-- agent:name ... -->
        val openPattern = Regex("""<!-- agent:${Regex.escape(component)}(\s[^>]*)? -->""")
        val closeTag = "<!-- /agent:$component -->"

        val codeRanges = findCodeBlockRanges(doc)

        // Find the first open tag match that is NOT inside a fenced code block
        val openMatch = openPattern.findAll(doc).firstOrNull { match ->
            codeRanges.none { range -> match.range.first >= range.first && match.range.first < range.second }
        } ?: return doc

        val contentStart = openMatch.range.last + 1

        // Find close tag that is also NOT inside a fenced code block
        var searchFrom = contentStart
        var closeIdx: Int
        while (true) {
            closeIdx = doc.indexOf(closeTag, searchFrom)
            if (closeIdx < 0) return doc
            if (codeRanges.none { range -> closeIdx >= range.first && closeIdx < range.second }) break
            searchFrom = closeIdx + closeTag.length
        }

        // Check mode from inline attributes: patch= takes precedence, mode= as fallback
        val attrs = openMatch.groupValues.getOrNull(1) ?: ""
        val patchMatch = Regex("""patch=(\w+)""").find(attrs)
        val modeMatch = Regex("""mode=(\w+)""").find(attrs)
        val mode = patchMatch?.groupValues?.getOrNull(1)
            ?: modeMatch?.groupValues?.getOrNull(1) ?: "replace"

        val before = doc.substring(0, contentStart)
        val existingContent = doc.substring(contentStart, closeIdx)
        val after = doc.substring(closeIdx)

        return when (mode) {
            "append" -> before + existingContent.trimEnd() + "\n" + content.trimEnd() + "\n" + after
            "prepend" -> before + "\n" + content.trimEnd() + "\n" + existingContent.trimStart() + after
            else -> before + "\n" + content.trimEnd() + "\n" + after // replace
        }
    }

    /**
     * Find byte ranges of fenced code blocks in the document.
     * Returns a list of (start, end) pairs where start is the offset of the opening
     * fence line and end is the offset just past the closing fence line.
     */
    private fun findCodeBlockRanges(doc: String): List<Pair<Int, Int>> {
        val ranges = mutableListOf<Pair<Int, Int>>()
        val fencePattern = Regex("""^[ \t]*```""", RegexOption.MULTILINE)
        var insideFence = false
        var fenceStart = 0

        for (match in fencePattern.findAll(doc)) {
            if (!insideFence) {
                fenceStart = match.range.first
                insideFence = true
            } else {
                // End of fenced block: include everything up to the end of the closing fence line
                val lineEnd = doc.indexOf('\n', match.range.last + 1)
                val blockEnd = if (lineEnd >= 0) lineEnd + 1 else doc.length
                ranges.add(Pair(fenceStart, blockEnd))
                insideFence = false
            }
        }

        return ranges
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
    val frontmatter: String?,
    val fullContent: String?,
)

data class ComponentPatch(
    val component: String,
    val content: String,
)

/**
 * Hand-written JSON parser for IPC patch files.
 * Avoids Gson dependency — Gson causes ClassNotFoundException at runtime
 * in some IntelliJ builds (see SlashCommandCompletionContributor removal).
 */
fun parsePatchJson(json: String): IpcPatch? {
    try {
        val file = extractStringField(json, "file") ?: return null
        val unmatched = extractStringField(json, "unmatched") ?: ""
        val frontmatter = extractStringField(json, "frontmatter")
        val fullContent = extractStringField(json, "fullContent")

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

        return IpcPatch(file, patches, unmatched, frontmatter, fullContent)
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
