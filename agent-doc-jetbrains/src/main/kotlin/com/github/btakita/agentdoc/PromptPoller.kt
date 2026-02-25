package com.github.btakita.agentdoc

import com.intellij.openapi.Disposable
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import java.util.concurrent.Executors
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit

/**
 * Polls `agent-doc prompt <file>` for active permission prompts.
 *
 * When a prompt is detected, shows a lightweight notification bar
 * at the top of the editor via [PromptPanel].
 */
class PromptPoller(private val project: Project) : Disposable {

    private val executor = Executors.newSingleThreadScheduledExecutor { r ->
        Thread(r, "agent-doc-prompt-poller").apply { isDaemon = true }
    }
    private var task: ScheduledFuture<*>? = null
    private var currentPrompt: PromptInfo? = null
    private var currentFile: VirtualFile? = null

    fun start(file: VirtualFile) {
        stop()
        currentFile = file
        val basePath = project.basePath ?: return
        val relativePath = TerminalUtil.relativePath(project, file)

        task = executor.scheduleWithFixedDelay({
            try {
                val info = pollPrompt(basePath, relativePath) ?: return@scheduleWithFixedDelay
                handlePromptResult(info, basePath, relativePath)
            } catch (_: Exception) {
                // Silently ignore polling errors
            }
        }, 500, 1500, TimeUnit.MILLISECONDS)
    }

    fun stop() {
        task?.cancel(false)
        task = null
        currentPrompt = null
        currentFile = null
        ApplicationManager.getApplication().invokeLater {
            PromptPanel.dismiss(project)
        }
    }

    override fun dispose() {
        stop()
        executor.shutdownNow()
    }

    private fun pollPrompt(basePath: String, relativePath: String): PromptInfo? {
        val agentDoc = TerminalUtil.resolveAgentDoc()
        val process = ProcessBuilder(agentDoc, "prompt", relativePath)
            .directory(java.io.File(basePath))
            .redirectErrorStream(false)
            .start()

        val stdout = process.inputStream.bufferedReader().readText()
        val exitCode = process.waitFor()
        if (exitCode != 0) return null

        return parsePromptJson(stdout)
    }

    private fun handlePromptResult(info: PromptInfo, basePath: String, relativePath: String) {
        if (info.active && info.options != null) {
            if (currentPrompt?.question != info.question) {
                currentPrompt = info
                ApplicationManager.getApplication().invokeLater {
                    PromptPanel.show(project, info) { optionIndex ->
                        answerPrompt(basePath, relativePath, optionIndex)
                    }
                }
            }
        } else if (!info.active && currentPrompt != null) {
            currentPrompt = null
            ApplicationManager.getApplication().invokeLater {
                PromptPanel.dismiss(project)
            }
        }
    }

    private fun answerPrompt(basePath: String, relativePath: String, optionIndex: Int) {
        currentPrompt = null
        Thread {
            try {
                val agentDoc = TerminalUtil.resolveAgentDoc()
                val process = ProcessBuilder(
                    agentDoc, "prompt", "--answer", optionIndex.toString(), relativePath
                )
                    .directory(java.io.File(basePath))
                    .redirectErrorStream(true)
                    .start()
                val output = process.inputStream.bufferedReader().readText()
                val exitCode = process.waitFor()
                if (exitCode != 0) {
                    TerminalUtil.notifyError(project, "agent-doc prompt --answer failed:\n$output")
                }
            } catch (e: Exception) {
                TerminalUtil.notifyError(project, "Failed to answer prompt: ${e.message}")
            }
        }.start()
    }

    companion object {
        private val instances = mutableMapOf<Project, PromptPoller>()

        fun getInstance(project: Project): PromptPoller {
            return instances.getOrPut(project) { PromptPoller(project) }
        }

        fun disposeProject(project: Project) {
            instances.remove(project)?.dispose()
        }
    }
}

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

private fun parsePromptJson(json: String): PromptInfo? {
    return try {
        // Minimal JSON parsing without external dependencies
        val trimmed = json.trim()
        if (!trimmed.startsWith("{")) return null

        val active = trimmed.contains("\"active\":true") || trimmed.contains("\"active\": true")
        if (!active) return PromptInfo(active = false)

        val question = extractJsonString(trimmed, "question")
        val selected = extractJsonInt(trimmed, "selected")
        val options = extractOptions(trimmed)

        PromptInfo(
            active = true,
            question = question,
            options = options,
            selected = selected,
        )
    } catch (_: Exception) {
        null
    }
}

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
