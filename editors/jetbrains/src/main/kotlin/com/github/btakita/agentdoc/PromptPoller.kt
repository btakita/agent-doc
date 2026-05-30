package com.github.btakita.agentdoc

import com.intellij.openapi.Disposable
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.openapi.vfs.VirtualFileManager
import com.intellij.openapi.vfs.newvfs.BulkFileListener
import com.intellij.openapi.vfs.newvfs.events.VFileContentChangeEvent
import com.intellij.openapi.vfs.newvfs.events.VFileEvent
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executors
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit

/**
 * Polls `agent-doc prompt --all` for active permission prompts across all sessions.
 *
 * Tracks all submitted files. Each poll cycle calls `agent-doc prompt --all` once,
 * refreshes tracked VirtualFiles, and shows a [PromptPanel] for any active prompt.
 */
class PromptPoller(private val project: Project) : Disposable {

    private val executor = Executors.newSingleThreadScheduledExecutor { r ->
        Thread(r, "agent-doc-prompt-poller").apply { isDaemon = true }
    }
    private var task: ScheduledFuture<*>? = null

    /** Files submitted via the plugin. Key = relative path, value = tracked state. */
    private val trackedFiles = ConcurrentHashMap<String, TrackedFile>()

    /** The currently displayed prompt (to avoid re-showing the same one). */
    @Volatile private var currentPromptKey: String? = null
    /** Cycle counter for periodic state logging. */
    private var cycleCount = 0L
    /** Whether the BulkFileListener has been registered. */
    private var fileListenerRegistered = false

    /** All active prompt keys from the last poll, for stable queue ordering. */
    @Volatile private var activePromptQueue: List<String> = emptyList()

    /** Recently answered prompt key — filtered from poll results until the answer takes effect. */
    @Volatile private var answeredPromptKey: String? = null

    /**
     * Start polling without registering a specific file.
     * Used on project open to detect prompts from sessions started outside the plugin.
     */
    fun startPolling() {
        ensurePolling()
    }

    /**
     * Register a file for tracking and ensure the poller is running.
     */
    fun addFile(file: VirtualFile) {
        val doc = FileDocumentManager.getInstance().getDocument(file)
        // Key by the canonical absolute path so the editor-registration path and
        // the poll-registration path (handlePollResults) converge on a single
        // entry per file. See trackingKey().
        trackedFiles[trackingKey(file.path)] = TrackedFile(file, file.modificationStamp, doc?.text)
        ensurePolling()
    }

    /**
     * Stop polling and clear all state.
     */
    fun stop() {
        task?.cancel(false)
        task = null
        trackedFiles.clear()
        currentPromptKey = null
        activePromptQueue = emptyList()
        answeredPromptKey = null
        ApplicationManager.getApplication().invokeLater {
            PromptPanel.dismiss(project)
        }
    }

    override fun dispose() {
        stop()
        executor.shutdownNow()
    }

    private fun ensurePolling() {
        if (task != null) return
        val basePath = project.basePath ?: return

        registerFileListener()
        LOG.info("[prompt-poller] starting polling for ${project.name}")
        task = executor.scheduleWithFixedDelay({
            try {
                val cycleStart = System.nanoTime()
                // Auto-save is best-effort — must not block prompt detection
                try { autoSaveTrackedFiles() } catch (_: Exception) {}
                val saveMs = (System.nanoTime() - cycleStart) / 1_000_000

                val refreshStart = System.nanoTime()
                refreshTrackedFiles()
                val refreshMs = (System.nanoTime() - refreshStart) / 1_000_000

                val pollStart = System.nanoTime()
                val entries = pollAll(basePath) ?: return@scheduleWithFixedDelay
                val pollMs = (System.nanoTime() - pollStart) / 1_000_000

                handlePollResults(entries, basePath)
                val totalMs = (System.nanoTime() - cycleStart) / 1_000_000
                if (totalMs > 200) LOG.info("[perf] prompt-poller cycle: ${totalMs}ms (save=${saveMs}ms refresh=${refreshMs}ms poll=${pollMs}ms tracked=${trackedFiles.size})")
                cycleCount++
                if (cycleCount % 40 == 0L) { // ~every 60s at 1.5s interval
                    val ffiTracked = AgentDocLib.get()?.agent_doc_tracked_count() ?: -1
                    LOG.info("[state] prompt-poller: trackedFiles=${trackedFiles.size} ffiTracked=$ffiTracked keys=[${trackedFiles.keys.joinToString(", ")}]")
                }
            } catch (e: Exception) {
                LOG.warn("[prompt-poller] poll error: ${e.message}")
            }
        }, 500, 1500, TimeUnit.MILLISECONDS)
    }

    /**
     * Listen for external disk writes to tracked files and merge immediately,
     * rather than waiting for the next 1.5s poll cycle.
     */
    private fun registerFileListener() {
        if (fileListenerRegistered) return
        fileListenerRegistered = true
        project.messageBus.connect(this).subscribe(VirtualFileManager.VFS_CHANGES, object : BulkFileListener {
            override fun after(events: List<VFileEvent>) {
                for (event in events) {
                    if (event !is VFileContentChangeEvent) continue
                    val file = event.file
                    val tracked = trackedFiles.values.find { it.file == file } ?: continue
                    tracked.lastKnownModStamp = file.modificationStamp
                    ApplicationManager.getApplication().invokeLater {
                        if (file.isValid) {
                            mergeOrReload(tracked)
                        }
                    }
                }
            }
        })
    }

    /**
     * Auto-save unsaved tracked documents so Claude sees the user's latest edits.
     * Uses invokeAndWait (blocking) to ensure saves complete before prompt polling.
     *
     * Refreshes VFS before saving to detect external disk writes (e.g. agent-doc
     * CRDT merge). If disk changed, merges first so we don't overwrite with a
     * stale in-memory buffer.
     */
    private fun autoSaveTrackedFiles() {
        ApplicationManager.getApplication().invokeAndWait {
            try {
                val fdm = FileDocumentManager.getInstance()
                for ((_, tracked) in trackedFiles) {
                    if (!tracked.file.isValid) continue
                    val doc = fdm.getDocument(tracked.file) ?: continue
                    if (fdm.isDocumentUnsaved(doc)) {
                        tracked.file.refresh(false, false)
                        val currentStamp = tracked.file.modificationStamp
                        if (currentStamp != tracked.lastKnownModStamp) {
                            mergeOrReload(tracked)
                            tracked.lastKnownModStamp = tracked.file.modificationStamp
                        }
                        if (fdm.isDocumentUnsaved(doc)) {
                            tracked.lastSavedContent = doc.text
                            fdm.saveDocument(doc)
                            tracked.lastKnownModStamp = tracked.file.modificationStamp
                        }
                    }
                }
            } catch (_: Exception) {
                // Best-effort save — never block prompt detection
            }
        }
    }

    /**
     * Refresh all tracked VirtualFiles — merge or reload if changed on disk.
     */
    private fun refreshTrackedFiles() {
        for ((_, tracked) in trackedFiles) {
            val file = tracked.file
            if (!file.isValid) continue

            file.refresh(false, false)
            val currentStamp = file.modificationStamp
            if (currentStamp != tracked.lastKnownModStamp) {
                tracked.lastKnownModStamp = currentStamp
                ApplicationManager.getApplication().invokeLater {
                    if (file.isValid) {
                        mergeOrReload(tracked)
                    }
                }
            }
        }
    }

    /**
     * Merge user's unsaved editor edits with external disk changes (Claude's response).
     *
     * Task 1 (PromptPoller fires during IPC write): PatchWatcher writes via Document API
     * then calls saveDocument(), changing the VFS modificationStamp. PromptPoller detects
     * this and calls mergeOrReload. When the doc has no unsaved changes and already matches
     * disk (normal after PatchWatcher), skip the reload — reloadFiles() would be a no-op
     * but triggers unnecessary IDE notifications.
     *
     * Task 2 (merge failure): preserve editor content instead of discarding via reloadFiles().
     *
     * Task 3 (FFI CRDT merge): use agent_doc_merge_crdt (conflict-free) with git merge-file
     * as fallback when FFI is unavailable.
     */
    private fun mergeOrReload(tracked: TrackedFile) {
        val fdm = FileDocumentManager.getInstance()
        val doc = fdm.getDocument(tracked.file) ?: return

        if (!fdm.isDocumentUnsaved(doc)) {
            // Task 1: doc is in sync with disk (e.g. PatchWatcher saved via Document API).
            // Only reload when doc content differs from disk (true external write bypassing Document API).
            val diskContent = String(tracked.file.contentsToByteArray(), Charsets.UTF_8)
            if (doc.text != diskContent) {
                fdm.reloadFiles(tracked.file)
            }
            return
        }

        val editorContent = doc.text
        val diskContent = String(tracked.file.contentsToByteArray(), Charsets.UTF_8)
        if (editorContent == diskContent) return

        val base = tracked.lastSavedContent ?: editorContent
        // Task 3: prefer FFI CRDT merge (conflict-free); fall back to git merge-file
        val merged = NativePatching.mergeCrdt(base, diskContent, editorContent)
            ?: gitMergeFile(base, diskContent, editorContent)
        if (merged != null) {
            ApplicationManager.getApplication().runWriteAction {
                doc.setText(merged)
            }
            tracked.lastSavedContent = merged
        } else {
            // Task 2: preserve editor content on merge failure — don't discard user edits
            TerminalUtil.notifyInfo(project, "Merge conflict with external write — editor content preserved.")
            tracked.lastSavedContent = editorContent  // reset base to avoid repeat merge attempts
        }
    }

    /**
     * 3-way merge via `git merge-file`. Returns merged content or null on conflict.
     * Used as fallback when FFI is unavailable.
     */
    private fun gitMergeFile(base: String, ours: String, theirs: String): String? {
        try {
            val baseFile = java.io.File.createTempFile("merge-base-", ".md")
            val oursFile = java.io.File.createTempFile("merge-ours-", ".md")
            val theirsFile = java.io.File.createTempFile("merge-theirs-", ".md")
            try {
                baseFile.writeText(base)
                oursFile.writeText(ours)
                theirsFile.writeText(theirs)

                val process = ProcessBuilder(
                    "git", "merge-file", "-p",
                    theirsFile.absolutePath, baseFile.absolutePath, oursFile.absolutePath
                )
                    .redirectErrorStream(false)
                    .start()
                val output = process.inputStream.bufferedReader().readText()
                val exitCode = process.waitFor()
                return if (exitCode == 0) output else null
            } finally {
                baseFile.delete()
                oursFile.delete()
                theirsFile.delete()
            }
        } catch (_: Exception) {
            return null
        }
    }

    /**
     * Call `agent-doc prompt --all` and parse the JSON array response.
     */
    private fun pollAll(basePath: String): List<PromptAllEntry>? {
        val agentDoc = TerminalUtil.resolveAgentDoc(basePath)
        val process = ProcessBuilder(agentDoc, "prompt", "--all")
            .directory(java.io.File(basePath))
            .redirectErrorStream(false)
            .start()

        val stdout = process.inputStream.bufferedReader().readText()
        val exitCode = process.waitFor()
        if (exitCode != 0) {
            LOG.debug("[prompt-poller] pollAll exit=$exitCode")
            return null
        }

        return parsePromptAllJson(stdout)
    }

    /**
     * Process poll results: show one active prompt at a time.
     * Sticks with the current prompt until it is resolved before advancing to the next.
     */
    private fun handlePollResults(entries: List<PromptAllEntry>, basePath: String) {
        // Resolve VirtualFiles for sessions we haven't explicitly tracked
        for (entry in entries) {
            if (entry.file.isEmpty()) continue
            val entryBasePath = entry.cwd?.takeIf { it.isNotBlank() } ?: basePath
            val absPath = java.io.File(entryBasePath, entry.file).absolutePath
            val vf = LocalFileSystem.getInstance().findFileByPath(absPath) ?: continue
            // Key by the resolved VirtualFile's canonical path (the same scheme
            // addFile uses) so a file already tracked from the editor side is not
            // re-added under a divergent key. Double-tracking previously created
            // two TrackedFile entries for one file, each with its own baseline,
            // and refreshTrackedFiles/autoSaveTrackedFiles then ran mergeOrReload
            // twice with divergent baselines — a full-document duplication path
            // (#ipcfullprompt). See trackingKey().
            val key = trackingKey(vf.path)
            if (!trackedFiles.containsKey(key)) {
                trackedFiles[key] = TrackedFile(vf, vf.modificationStamp)
            }
        }

        // Collect all active prompts keyed by owning root, file, and question.
        val allActiveEntries = entries.filter { it.info.active && it.info.options != null }
        if (allActiveEntries.isNotEmpty()) {
            LOG.info("[prompt-poller] ${allActiveEntries.size} active prompt(s) detected")
        }
        val allActiveByKey = allActiveEntries.associateBy {
            "${it.cwd?.takeIf { cwd -> cwd.isNotBlank() } ?: basePath}:${it.file}:${it.info.question}"
        }

        // Clear answered key once the answer takes effect (prompt disappears from poll)
        if (answeredPromptKey != null && answeredPromptKey !in allActiveByKey.keys) {
            answeredPromptKey = null
        }

        // Filter out recently-answered prompt (grace period until answer takes effect)
        val activeByKey = if (answeredPromptKey != null) {
            allActiveByKey.filterKeys { it != answeredPromptKey }
        } else {
            allActiveByKey
        }
        val activeKeys = activeByKey.keys

        if (activeKeys.isEmpty()) {
            if (currentPromptKey != null) {
                currentPromptKey = null
                activePromptQueue = emptyList()
                ApplicationManager.getApplication().invokeLater {
                    PromptPanel.dismiss(project)
                }
            }
            return
        }

        // If the current prompt is still active, keep showing it (no flicker)
        if (currentPromptKey != null && currentPromptKey in activeKeys) {
            activePromptQueue = activeKeys.toList()
            return
        }

        // Current prompt was resolved or nothing is showing — pick next
        val nextKey = activePromptQueue
            .firstOrNull { it in activeKeys && it != currentPromptKey }
            ?: activeKeys.first()

        val next = activeByKey[nextKey] ?: return
        currentPromptKey = nextKey
        activePromptQueue = activeKeys.toList()

        val fileName = next.file.substringAfterLast('/').ifEmpty { next.file }
        val filePath = next.file
        val totalActive = activeKeys.size

        LOG.info("[prompt-poller] showing prompt: $nextKey ($totalActive active)")
        ApplicationManager.getApplication().invokeLater {
            PromptPanel.show(project, next.info, fileName, totalActive) { optionIndex ->
                answerPrompt(basePath, next.cwd, filePath, optionIndex)
            }
        }
    }

    private fun answerPrompt(basePath: String, promptCwd: String?, relativePath: String, optionIndex: Int) {
        val promptKey = currentPromptKey
        answeredPromptKey = promptKey
        currentPromptKey = null
        Thread {
            try {
                val cwd = promptCwd?.takeIf { it.isNotBlank() } ?: basePath
                val agentDoc = TerminalUtil.resolveAgentDoc(cwd)
                val result = runPromptAnswerCommand(agentDoc, cwd, relativePath, optionIndex)
                if (result.exitCode != 0) {
                    answeredPromptKey = null
                    TerminalUtil.notifyError(project, "agent-doc prompt --answer failed:\n${result.output}")
                }
            } catch (e: Exception) {
                answeredPromptKey = null
                TerminalUtil.notifyError(project, "Failed to answer prompt: ${e.message}")
            }
        }.start()
    }

    companion object {
        private val LOG = com.intellij.openapi.diagnostic.Logger.getInstance(PromptPoller::class.java)
        private val instances = mutableMapOf<Project, PromptPoller>()

        fun getInstance(project: Project): PromptPoller {
            return instances.getOrPut(project) { PromptPoller(project) }
        }

        fun disposeProject(project: Project) {
            instances.remove(project)?.dispose()
        }

        fun disposeAll() {
            instances.values.toList().forEach { it.dispose() }
            instances.clear()
        }
    }
}

private class TrackedFile(
    val file: VirtualFile,
    var lastKnownModStamp: Long,
    var lastSavedContent: String? = null,
)

data class PromptInfo(
    val active: Boolean,
    val question: String? = null,
    val options: List<PromptOption>? = null,
    val selected: Int? = null,
)

data class PromptOption(
    val index: Int,
    val label: String,
)

data class PromptAllEntry(
    val sessionId: String,
    val file: String,
    val cwd: String? = null,
    val info: PromptInfo,
)

internal data class PromptAnswerResult(
    val exitCode: Int,
    val output: String,
)

internal fun buildPromptAnswerCommand(
    agentDoc: String,
    optionIndex: Int,
    relativePath: String,
): List<String> = listOf(agentDoc, "prompt", "--answer", optionIndex.toString(), relativePath)

internal fun runPromptAnswerCommand(
    agentDoc: String,
    cwd: String,
    relativePath: String,
    optionIndex: Int,
): PromptAnswerResult {
    val process = ProcessBuilder(buildPromptAnswerCommand(agentDoc, optionIndex, relativePath))
        .directory(java.io.File(cwd))
        .redirectErrorStream(true)
        .start()
    val output = process.inputStream.bufferedReader().readText()
    val exitCode = process.waitFor()
    return PromptAnswerResult(exitCode, output)
}

// ---------------------------------------------------------------------------
// JSON parsing for `agent-doc prompt --all` (JSON array)
// ---------------------------------------------------------------------------

internal fun parsePromptAllJson(json: String): List<PromptAllEntry>? {
    return try {
        val trimmed = json.trim()
        if (!trimmed.startsWith("[")) return null

        val entries = mutableListOf<PromptAllEntry>()

        // Split the array into individual objects
        var depth = 0
        var objStart = -1
        for (i in trimmed.indices) {
            when (trimmed[i]) {
                '{' -> {
                    if (depth == 0) objStart = i
                    depth++
                }
                '}' -> {
                    depth--
                    if (depth == 0 && objStart >= 0) {
                        val obj = trimmed.substring(objStart, i + 1)
                        parsePromptAllEntry(obj)?.let { entries.add(it) }
                        objStart = -1
                    }
                }
            }
        }

        entries
    } catch (_: Exception) {
        null
    }
}

private fun parsePromptAllEntry(json: String): PromptAllEntry? {
    val sessionId = extractJsonString(json, "session_id") ?: return null
    val file = extractJsonString(json, "file") ?: ""
    val cwd = extractJsonString(json, "cwd")
    val active = json.contains("\"active\":true") || json.contains("\"active\": true")

    val info = if (active) {
        val question = extractJsonString(json, "question")
        val selected = extractJsonInt(json, "selected")
        val options = extractOptions(json)
        PromptInfo(active = true, question = question, options = options, selected = selected)
    } else {
        PromptInfo(active = false)
    }

    return PromptAllEntry(sessionId = sessionId, file = file, cwd = cwd, info = info)
}

/**
 * Canonical key for a tracked session document. Both registration paths —
 * [PromptPoller.addFile] (editor side, keyed from `VirtualFile.path`) and
 * `handlePollResults` (poll side, keyed from the resolved `VirtualFile.path`)
 * — must produce the same key for the same file so it is tracked exactly once.
 *
 * Previously addFile keyed by the project-relative path while the poller keyed
 * by `"$cwd:$file"`, so the same file was tracked under two keys. That created
 * two independent `TrackedFile` baselines, and `refreshTrackedFiles` /
 * `autoSaveTrackedFiles` then invoked `mergeOrReload` twice per change with
 * divergent baselines — a full-document duplication path (#ipcfullprompt).
 */
internal fun trackingKey(absolutePath: String): String = absolutePath

private fun extractJsonString(json: String, key: String): String? {
    val pattern = "\"$key\"\\s*:\\s*\"".toRegex()
    val match = pattern.find(json) ?: return null
    val start = match.range.last + 1
    val sb = StringBuilder()
    var i = start
    while (i < json.length) {
        val c = json[i]
        if (c == '\\' && i + 1 < json.length) {
            sb.append(json[i + 1])
            i += 2
        } else if (c == '"') {
            break
        } else {
            sb.append(c)
            i++
        }
    }
    return sb.toString()
}

private fun extractJsonInt(json: String, key: String): Int? {
    val pattern = "\"$key\"\\s*:\\s*(\\d+)".toRegex()
    return pattern.find(json)?.groupValues?.get(1)?.toIntOrNull()
}

private fun extractOptions(json: String): List<PromptOption>? {
    val optionsStart = json.indexOf("\"options\"")
    if (optionsStart < 0) return null

    val arrayStart = json.indexOf('[', optionsStart)
    if (arrayStart < 0) return null

    val arrayEnd = json.indexOf(']', arrayStart)
    if (arrayEnd < 0) return null

    val arrayContent = json.substring(arrayStart + 1, arrayEnd)
    val options = mutableListOf<PromptOption>()

    // Find each {index:N, label:"..."} object
    var pos = 0
    while (pos < arrayContent.length) {
        val objStart = arrayContent.indexOf('{', pos)
        if (objStart < 0) break
        val objEnd = arrayContent.indexOf('}', objStart)
        if (objEnd < 0) break

        val obj = arrayContent.substring(objStart, objEnd + 1)
        val index = extractJsonInt(obj, "index")
        val label = extractJsonString(obj, "label")
        if (index != null && label != null) {
            options.add(PromptOption(index, label))
        }
        pos = objEnd + 1
    }

    return if (options.isEmpty()) null else options
}
