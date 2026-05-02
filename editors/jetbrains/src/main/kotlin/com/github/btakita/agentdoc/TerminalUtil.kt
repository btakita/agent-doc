package com.github.btakita.agentdoc

import com.intellij.codeInsight.hint.HintManager
import com.intellij.notification.Notification
import com.intellij.notification.NotificationGroupManager
import com.intellij.notification.NotificationType
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.OpenFileDescriptor
import com.intellij.openapi.ide.CopyPasteManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.openapi.vfs.VirtualFile
import java.awt.datatransfer.StringSelection
import java.io.File

object TerminalUtil {

    fun relativePath(project: Project, file: VirtualFile): String {
        val basePath = project.basePath ?: return file.path
        return if (file.path.startsWith(basePath)) {
            file.path.removePrefix("$basePath/")
        } else {
            file.path
        }
    }

    /**
     * Resolve the agent-doc project root for [file].
     *
     * Walks up from the file's parent looking for the nearest ancestor with
     * `.agent-doc/` (via the shared FFI helper). If the file lives inside a
     * submodule that is itself an agent-doc project (e.g. `src/session-share/`),
     * the submodule root is returned. Otherwise falls back to the IDE project's
     * `basePath`.
     *
     * Returns `(projectRoot, relativePath)` where `relativePath` is `file.path`
     * relative to `projectRoot`, suitable for passing to `agent-doc` commands
     * run from that directory.
     */
    fun resolveProject(project: Project, file: VirtualFile): Pair<String, String> {
        val basePath = project.basePath
        val ffi = NativePatching.resolveProjectPath(file.path)
        if (ffi != null) {
            // Register resolved root with PatchWatcher on-demand. This handles submodule
            // roots that weren't present at startup (e.g. user opens a file in a freshly
            // cloned submodule). Idempotent — no-op if already registered.
            if (basePath != null && ffi.first != basePath) {
                try {
                    PatchWatcher.getInstance(project).registerRoot(ffi.first)
                } catch (_: Exception) { /* best-effort */ }
            }
            return ffi
        }
        // FFI unavailable or no `.agent-doc/` ancestor — fall back to workspace basePath.
        if (basePath != null) {
            return Pair(basePath, relativePath(project, file))
        }
        return Pair(java.io.File(file.path).parent ?: "/", java.io.File(file.path).name)
    }

    /**
     * Routes a document trigger command via `agent-doc route --dispatch-only`.
     *
     * This calls `agent-doc route --dispatch-only <path>` which:
     * 1. Reads the session UUID from the file's frontmatter
     * 2. Looks up the tmux pane for that session
     * 3. Resolves the active harness trigger and sends the bare reopen via `tmux send-keys`
     * 4. Auto-starts a new agent session if needed
     */
    fun sendToTerminal(project: Project, file: VirtualFile, onComplete: (() -> Unit)? = null) {
        val LOG = com.intellij.openapi.diagnostic.Logger.getInstance(TerminalUtil::class.java)
        val (cwd, relativePath) = resolveProject(project, file)
        val agentDoc = resolveAgentDoc(cwd)

        LOG.warn("[route] sendToTerminal: cwd=$cwd rel=$relativePath binary=$agentDoc")

        // is_busy guard removed: no production code sets the status signals,
        // so the guard only produced false positives (blocked every route attempt)

        try {
            // Build route command with optional layout args
            val cmd = mutableListOf(agentDoc, "route", "--dispatch-only", relativePath)

            // Only include visible files that live under the same project root
            // as the focused file — sibling submodules have their own sessions.
            val manager = com.intellij.openapi.fileEditor.FileEditorManager.getInstance(project)
            val cwdPrefix = "$cwd/"
            fun underProject(vf: VirtualFile): Boolean =
                vf.path == cwd || vf.path.startsWith(cwdPrefix)
            fun relTo(vf: VirtualFile): String =
                if (vf.path == cwd) vf.name else vf.path.removePrefix(cwdPrefix)

            val visibleMdFiles = manager.selectedFiles
                .filter { it.name.endsWith(".md") && underProject(it) }
                .map { relTo(it) }
                .distinct()

            if (visibleMdFiles.size > 1) {
                val editorLayout = LayoutDetector.detectEditorLayout(project)
                if (editorLayout != null && editorLayout.columns.size > 1) {
                    for (col in editorLayout.columns) {
                        cmd.addAll(listOf("--col", col.files.joinToString(",")))
                    }
                } else {
                    cmd.addAll(listOf("--col", visibleMdFiles.joinToString(",")))
                }
            } else if (visibleMdFiles.size == 1) {
                cmd.addAll(listOf("--col", visibleMdFiles[0]))
            }

            // Pass focused file
            val focusedFile = manager.selectedTextEditor?.virtualFile?.let {
                if (it.name.endsWith(".md") && underProject(it)) relTo(it) else null
            }
            if (focusedFile != null) {
                cmd.addAll(listOf("--focus", focusedFile))
            }

            LOG.warn("[route] executing: ${cmd.joinToString(" ")}")

            val process = ProcessBuilder(cmd)
                .directory(java.io.File(cwd))
                .redirectErrorStream(true)
                .start()

            val startedAt = System.currentTimeMillis()
            val progress = startProgressNotification(project, "Routing $relativePath...")
            showHint(project, "Routing $relativePath...")

            Thread {
                try {
                    val output = process.inputStream.bufferedReader().readText()
                    val exitCode = process.waitFor()
                    val elapsed = formatElapsedMillis(System.currentTimeMillis() - startedAt)
                    if (exitCode != 0) {
                        LOG.warn("[route] FAILED (exit $exitCode): $output")
                        notifyError(
                            project,
                            "agent-doc route failed for $relativePath after $elapsed (exit $exitCode):\n$output"
                        )
                    } else {
                        LOG.warn("[route] SUCCESS: $output")
                        showHint(project, "Routed $relativePath in $elapsed")
                    }
                } finally {
                    progress?.expire()
                    onComplete?.invoke()
                }
            }.start()
        } catch (e: Exception) {
            onComplete?.invoke()
            notifyError(project, "Failed to run agent-doc: ${e.message}\nLooked for: $agentDoc")
        }
    }

    /**
     * Runs `agent-doc fix <path>` for the active markdown document.
     *
     * This is the editor-side recovery path for a document whose tmux/session
     * ownership metadata or live pane state needs deterministic repair before
     * another routed reopen is attempted.
     */
    fun fixDocument(project: Project, file: VirtualFile, onComplete: (() -> Unit)? = null) {
        val (cwd, relativePath) = resolveProject(project, file)
        val agentDoc = resolveAgentDoc(cwd)
        try {
            val process = ProcessBuilder(agentDoc, "fix", relativePath)
                .directory(java.io.File(cwd))
                .redirectErrorStream(true)
                .start()

            showHint(project, "Fixing $relativePath")

            Thread {
                try {
                    val output = process.inputStream.bufferedReader().readText()
                    val exitCode = process.waitFor()
                    if (exitCode != 0) {
                        notifyError(project, "agent-doc fix failed (exit $exitCode):\n$output")
                    } else {
                        showHint(project, output.trim().ifEmpty { "Fixed $relativePath" })
                    }
                } finally {
                    onComplete?.invoke()
                }
            }.start()
        } catch (e: Exception) {
            onComplete?.invoke()
            notifyError(project, "Failed to run agent-doc fix: ${e.message}\nLooked for: $agentDoc")
        }
    }

    /**
     * Runs a document session via `agent-doc run --agent <agent>`.
     *
     * This calls `agent-doc run --agent <agent> <path>` which:
     * 1. Computes the diff for the document
     * 2. Builds a prompt for the agent
     * 3. Sends the prompt to the specified agent backend
     * 4. Updates the document with the response
     */
    fun runWithAgent(project: Project, agent: String, file: VirtualFile, onComplete: (() -> Unit)? = null) {
        val (cwd, relativePath) = resolveProject(project, file)
        val agentDoc = resolveAgentDoc(cwd)
        try {
            val process = ProcessBuilder(agentDoc, "run", "--agent", agent, relativePath)
                .directory(java.io.File(cwd))
                .redirectErrorStream(true)
                .start()

            // Show quick inline hint near cursor
            showHint(project, "Running with $agent: $relativePath")

            // Read output in background thread to avoid blocking EDT
            Thread {
                try {
                    val output = process.inputStream.bufferedReader().readText()
                    val exitCode = process.waitFor()
                    if (exitCode != 0) {
                        notifyError(project, "agent-doc run failed (exit $exitCode):\n$output")
                    } else {
                        // Notify success and expire quickly
                        notifyInfo(project, "Agent $agent finished: $relativePath")
                    }

                    // For Junie agent, open the request file in the editor so the user (or Junie agent) sees the diff
                    if (agent == "junie") {
                        openJunieRequest(project)
                    }
                } finally {
                    onComplete?.invoke()
                }
            }.start()
        } catch (e: Exception) {
            onComplete?.invoke()
            notifyError(project, "Failed to run agent-doc: ${e.message}\nLooked for: $agentDoc")
        }
    }

    fun resolveAgentDoc(basePath: String? = null): String {
        val candidates = listOfNotNull(
            basePath?.let { "$it/.bin/agent-doc" },
            System.getenv("HOME")?.let { "$it/bin/agent-doc" },
            System.getenv("HOME")?.let { "$it/.local/bin/agent-doc" },
            System.getenv("HOME")?.let { "$it/.cargo/bin/agent-doc" },
            "/usr/local/bin/agent-doc"
        )
        for (path in candidates) {
            if (java.io.File(path).canExecute()) {
                return path
            }
        }
        return "agent-doc"
    }

    fun showHint(project: Project, message: String) {
        ApplicationManager.getApplication().invokeLater {
            val editor = FileEditorManager.getInstance(project).selectedTextEditor ?: return@invokeLater
            HintManager.getInstance().showInformationHint(editor, message)
        }
    }

    fun notifyError(project: Project, content: String) {
        try {
            NotificationGroupManager.getInstance()
                .getNotificationGroup("Agent Doc")
                .createNotification(content, NotificationType.ERROR)
                .notify(project)
        } catch (_: Exception) {
            System.err.println("[agent-doc] $content")
        }
    }

    /**
     * Reads sessions.json and returns the tmux window ID from any session
     * entry that belongs to this project (matching cwd). Returns null if
     * no window is recorded or sessions.json doesn't exist.
     */
    fun projectWindowId(project: Project): String? {
        // Find the "agent-doc" window by name in any tmux session.
        // This is more reliable than reading window IDs from sessions.json,
        // which become stale when windows are recreated.
        try {
            val process = ProcessBuilder(
                "tmux", "list-windows", "-a",
                "-F", "#{window_id} #{window_name}"
            ).redirectErrorStream(false).start()
            val output = process.inputStream.bufferedReader().readText()
            process.waitFor()
            for (line in output.lines()) {
                val parts = line.split(" ", limit = 2)
                if (parts.size == 2 && parts[1] == "agent-doc") {
                    return parts[0] // e.g. "@46"
                }
            }
        } catch (_: Exception) {
            // Fall through
        }
        return null
    }

    /**
     * Extracts a brief layout description from a command list.
     * Returns a string like "--col a.md,b.md --col c.md" or "focus a.md",
     * suitable for showing in a notification balloon.
     */
    fun formatLayoutSummary(cmd: List<String>): String {
        // Find the subcommand (sync or focus)
        val subcommand = cmd.getOrNull(1) ?: return cmd.joinToString(" ")
        return when (subcommand) {
            "sync" -> {
                val parts = mutableListOf<String>()
                var focusFile: String? = null
                var i = 2
                while (i < cmd.size) {
                    if (cmd[i] == "--col" && i + 1 < cmd.size) {
                        parts.add("--col ${cmd[i + 1]}")
                        i += 2
                    } else if (cmd[i] == "--focus" && i + 1 < cmd.size) {
                        focusFile = cmd[i + 1]
                        i += 2
                    } else {
                        i++
                    }
                }
                val focusSuffix = if (focusFile != null) " [focus: $focusFile]" else ""
                "Sync: ${parts.joinToString(" ")}$focusSuffix"
            }
            "focus" -> "Focus: ${cmd.getOrNull(2) ?: ""}"
            else -> cmd.drop(1).joinToString(" ")
        }
    }

    fun notifyInfo(project: Project, content: String) {
        try {
            val notification = NotificationGroupManager.getInstance()
                .getNotificationGroup("Agent Doc")
                .createNotification(content, NotificationType.INFORMATION)
            notification.notify(project)
            // Auto-expire after 3 seconds
            Thread {
                Thread.sleep(3000)
                notification.expire()
            }.start()
        } catch (_: Exception) {
            System.err.println("[agent-doc] $content")
        }
    }

    fun startProgressNotification(project: Project, content: String): Notification? {
        return try {
            val notification = NotificationGroupManager.getInstance()
                .getNotificationGroup("Agent Doc")
                .createNotification(content, NotificationType.INFORMATION)
            notification.notify(project)
            notification
        } catch (_: Exception) {
            System.err.println("[agent-doc] $content")
            null
        }
    }

    private fun formatElapsedMillis(elapsedMs: Long): String {
        val seconds = elapsedMs / 1000.0
        return if (seconds >= 10.0) {
            String.format("%.0fs", seconds)
        } else {
            String.format("%.1fs", seconds)
        }
    }

    /**
     * Opens the ~/.cache/junie-bridge/request.md file in the editor.
     * This file is written by junie-bridge.sh and contains the diff/prompt for Junie.
     */
    private fun openJunieRequest(project: Project) {
        val home = System.getProperty("user.home") ?: return
        val requestPath = "$home/.cache/junie-bridge/request.md"
        val requestFile = File(requestPath)
        if (!requestFile.exists()) return

        ApplicationManager.getApplication().invokeLater {
            val virtualFile = LocalFileSystem.getInstance().refreshAndFindFileByIoFile(requestFile)
            if (virtualFile != null) {
                // Open and focus the file
                FileEditorManager.getInstance(project).openTextEditor(
                    OpenFileDescriptor(project, virtualFile),
                    true
                )
                
                // Copy the diff content to clipboard to make it even easier to send to Junie
                try {
                    val content = requestFile.readText()
                    CopyPasteManager.getInstance().setContents(StringSelection(content))
                    showHint(project, "Opened Junie request (diff copied to clipboard)")
                } catch (e: Exception) {
                    showHint(project, "Opened Junie request")
                }
            }
        }
    }
}
