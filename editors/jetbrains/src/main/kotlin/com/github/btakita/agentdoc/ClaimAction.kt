package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.fileEditor.ex.FileEditorManagerEx
import com.intellij.openapi.project.Project
import com.intellij.openapi.ui.Messages

/**
 * Action that claims the focused .md file and syncs the tmux layout.
 *
 * Triggered by Ctrl+Shift+Alt+C (configurable in Keymap settings).
 * 1. Runs `agent-doc claim <file>` on the focused file (adds frontmatter if missing)
 * 2. Runs SyncLayoutAction.syncLayout() to arrange tmux panes
 *
 * The claim step is essential for unclaimed files — it generates the session UUID
 * and scaffolds components. Sync alone can't manage files without session UUIDs.
 *
 * Cross-session reject UX: when the focused pane lives in a tmux session other
 * than the configured project session, `agent-doc claim` exits non-zero and emits
 * a structured `[claim] cross-session-reject ...` marker (claim.rs). Instead of
 * surfacing the raw exit-1 text, this action parses the marker and prompts the
 * user with Force Claim / Switch Project Session / Cancel.
 */
class ClaimAction : AnAction() {

    companion object {
        private val LOG = Logger.getInstance(ClaimAction::class.java)

        /** Stable marker prefix emitted by `agent-doc claim` on a cross-session reject. */
        const val CROSS_SESSION_REJECT_MARKER = "[claim] cross-session-reject"

        /** Parsed cross-session-reject signal from `agent-doc claim` stderr. */
        data class CrossSessionReject(
            val paneId: String,
            val paneSession: String,
            val configured: String,
        )

        /**
         * Parse the structured cross-session-reject marker emitted by
         * `agent-doc claim` (claim.rs `cross_session_reject_marker`). Returns null
         * when the output carries no such marker or is missing a field. Field
         * order in the marker is stable (`pane_id`, `pane_session`, `configured`)
         * but this parser is order-independent.
         */
        fun parseCrossSessionReject(output: String): CrossSessionReject? {
            val line = output.lineSequence()
                .firstOrNull { it.contains(CROSS_SESSION_REJECT_MARKER) } ?: return null
            val fields = line.substringAfter(CROSS_SESSION_REJECT_MARKER)
                .trim()
                .split(Regex("\\s+"))
                .mapNotNull { tok ->
                    val eq = tok.indexOf('=')
                    if (eq <= 0) null else tok.substring(0, eq) to tok.substring(eq + 1)
                }
                .toMap()
            val paneId = fields["pane_id"] ?: return null
            val paneSession = fields["pane_session"] ?: return null
            val configured = fields["configured"] ?: return null
            return CrossSessionReject(paneId, paneSession, configured)
        }
    }

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE) ?: return

        val (cwd, relativePath) = TerminalUtil.resolveProject(project, file)

        // Determine position from editor layout so claim targets the correct tmux pane.
        // Without --position, claim falls back to the last active tmux pane (wrong).
        // LayoutDetector maps horizontal splits to left/right columns, vertical to same column.
        // LayoutDetector still uses workspace-relative paths, so compare against those.
        val layoutRelPath = TerminalUtil.relativePath(project, file)
        val managerEx = FileEditorManagerEx.getInstanceEx(project)
        val editorLayout = if (managerEx.windows.size > 1)
            LayoutDetector.detectEditorLayout(project) else null
        val position = editorLayout?.let { layout ->
            val colIdx = layout.columns.indexOfFirst { col -> layoutRelPath in col.files }
            when {
                colIdx < 0 -> null                         // file not found in layout
                colIdx == 0 -> "left"
                colIdx == layout.columns.size - 1 -> "right"
                else -> null                               // middle column — skip for now
            }
        }

        Thread {
            try {
                val agentDoc = TerminalUtil.resolveAgentDoc(cwd)
                val (exitCode, output) = runClaim(cwd, agentDoc, relativePath, position, force = false)
                if (exitCode == 0) {
                    TerminalUtil.showHint(project, output.ifEmpty { "Claimed $relativePath" })
                    SyncLayoutAction.syncLayout(project, notify = false, noAutostart = false)
                    return@Thread
                }
                val reject = parseCrossSessionReject(output)
                if (reject != null) {
                    promptCrossSessionChoice(project, cwd, agentDoc, relativePath, position, reject)
                } else {
                    TerminalUtil.notifyError(project, "Claim failed (exit $exitCode):\n$output")
                    SyncLayoutAction.syncLayout(project, notify = false, noAutostart = false)
                }
            } catch (ex: Exception) {
                TerminalUtil.notifyError(project, "Failed to run agent-doc claim: ${ex.message}")
            }
        }.start()
    }

    /** Run `agent-doc claim <rel> [--force] [--position P]`; returns (exitCode, merged output). */
    private fun runClaim(
        cwd: String,
        agentDoc: String,
        relativePath: String,
        position: String?,
        force: Boolean,
    ): Pair<Int, String> {
        val cmd = mutableListOf(agentDoc, "claim", relativePath)
        if (force) cmd.add("--force")
        if (position != null) cmd.addAll(listOf("--position", position))
        LOG.debug("claim: ${cmd.joinToString(" ")}")
        val process = ProcessBuilder(cmd)
            .directory(java.io.File(cwd))
            .redirectErrorStream(true)
            .start()
        val output = process.inputStream.bufferedReader().readText().trim()
        return process.waitFor() to output
    }

    /** Run `agent-doc session set <session>`; returns (exitCode, merged output). */
    private fun runSessionSet(cwd: String, agentDoc: String, session: String): Pair<Int, String> {
        val process = ProcessBuilder(listOf(agentDoc, "session", "set", session))
            .directory(java.io.File(cwd))
            .redirectErrorStream(true)
            .start()
        val output = process.inputStream.bufferedReader().readText().trim()
        return process.waitFor() to output
    }

    /** Show the 3-choice dialog on the EDT and dispatch the chosen recovery. */
    private fun promptCrossSessionChoice(
        project: Project,
        cwd: String,
        agentDoc: String,
        relativePath: String,
        position: String?,
        reject: CrossSessionReject,
    ) {
        ApplicationManager.getApplication().invokeLater {
            val choice = Messages.showDialog(
                project,
                "Pane ${reject.paneId} is in tmux session '${reject.paneSession}', but this project's " +
                    "configured session is '${reject.configured}'.\n\nHow do you want to claim it?",
                "Claim for Tmux Pane — Cross-Session",
                arrayOf("Force Claim", "Switch Project Session", "Cancel"),
                0,
                Messages.getQuestionIcon(),
            )
            when (choice) {
                0 -> runClaimChoice(project, cwd, agentDoc, relativePath, position, switchTo = null, force = true)
                1 -> runClaimChoice(
                    project, cwd, agentDoc, relativePath, position,
                    switchTo = reject.paneSession, force = false,
                )
                else -> { /* Cancel — leave the file unclaimed, no mutation */ }
            }
        }
    }

    /**
     * Off-EDT: optionally switch the project session, then (re-)claim. `force`
     * re-runs claim with `--force`; `switchTo` first runs `session set` so the
     * configured session matches the pane before a normal claim.
     */
    private fun runClaimChoice(
        project: Project,
        cwd: String,
        agentDoc: String,
        relativePath: String,
        position: String?,
        switchTo: String?,
        force: Boolean,
    ) {
        Thread {
            try {
                if (switchTo != null) {
                    val (setExit, setOut) = runSessionSet(cwd, agentDoc, switchTo)
                    if (setExit != 0) {
                        TerminalUtil.notifyError(project, "Switch project session failed (exit $setExit):\n$setOut")
                        return@Thread
                    }
                }
                val (exitCode, output) = runClaim(cwd, agentDoc, relativePath, position, force = force)
                if (exitCode == 0) {
                    TerminalUtil.showHint(project, output.ifEmpty { "Claimed $relativePath" })
                } else {
                    TerminalUtil.notifyError(project, "Claim failed (exit $exitCode):\n$output")
                }
                SyncLayoutAction.syncLayout(project, notify = false, noAutostart = false)
            } catch (ex: Exception) {
                TerminalUtil.notifyError(project, "Failed to run agent-doc claim: ${ex.message}")
            }
        }.start()
    }

    override fun update(e: AnActionEvent) {
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE)
        e.presentation.isEnabledAndVisible =
            file != null && file.extension?.lowercase() == "md"
    }
}
