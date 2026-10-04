package com.github.btakita.agentdoc

import com.google.gson.JsonElement
import com.google.gson.JsonObject
import com.google.gson.JsonParser
import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.ide.CopyPasteManager
import com.intellij.openapi.project.DumbAware
import com.intellij.openapi.project.Project
import com.intellij.openapi.ui.Messages
import java.awt.datatransfer.StringSelection

/** Editor kind this plugin registers as; selects `expected_plugins.<kind>` in build info. */
internal const val ABOUT_EDITOR_KIND = "jetbrains"
private const val ABOUT_CLI_TIMEOUT_MS = 10_000L

/** `editoractionmenu`: what the loaded native generation reports, without loading anything. */
internal data class NativeAboutSnapshot(
    val path: String?,
    val loadTarget: String?,
    val version: String?,
    val buildInfoJson: String?,
    val loadError: String?,
)

/** One agent-doc component's build identity (`agent-doc-build-info-v1`, or version text). */
internal data class AgentDocBuildFacts(
    val version: String?,
    val buildId: String? = null,
    val executable: String? = null,
    val library: String? = null,
    val expectedPluginVersion: String? = null,
)

/** Everything About Agent Doc shows. Pure data so the report and warnings are unit-testable. */
internal data class AboutAgentDocFacts(
    val pluginVersion: String,
    val cliCommand: String,
    val cli: AgentDocBuildFacts?,
    val cliNote: String?,
    val nativePath: String?,
    val native: AgentDocBuildFacts?,
    val nativeNote: String?,
)

private fun JsonObject.stringOrNull(key: String): String? {
    val element: JsonElement = get(key) ?: return null
    return if (element.isJsonPrimitive) element.asString.takeIf { it.isNotBlank() } else null
}

/** Parse `agent-doc version --json` / `agent_doc_build_info_json()`; null when not that contract. */
internal fun parseBuildInfoJson(json: String, editorKind: String = ABOUT_EDITOR_KIND): AgentDocBuildFacts? {
    val root =
        try {
            JsonParser.parseString(json.trim())
        } catch (_: Exception) {
            return null
        }
    if (!root.isJsonObject) return null
    val obj = root.asJsonObject
    if (obj.stringOrNull("schema")?.startsWith("agent-doc-build-info-") != true) return null
    val expected = obj.get("expected_plugins")?.takeIf { it.isJsonObject }?.asJsonObject
    return AgentDocBuildFacts(
        version = obj.stringOrNull("version"),
        buildId = obj.stringOrNull("build_id"),
        executable = obj.stringOrNull("executable"),
        library = obj.stringOrNull("library"),
        expectedPluginVersion = expected?.stringOrNull(editorKind),
    )
}

/** Parse `agent-doc --version` (`agent-doc 0.35.451`), the fallback for older binaries. */
internal fun parseAgentDocVersionText(text: String): String? {
    val line = text.lineSequence().map { it.trim() }.firstOrNull { it.isNotEmpty() } ?: return null
    val parts = line.split(Regex("\\s+"))
    if (parts.size < 2 || !parts[0].startsWith("agent-doc")) return null
    return parts[1].takeIf { it.firstOrNull()?.isDigit() == true }
}

/** Mismatches worth a warning: each one names both sides and what to do. */
internal fun aboutMismatches(facts: AboutAgentDocFacts): List<String> = buildList {
    val cli = facts.cli
    val native = facts.native
    if (cli == null) {
        add("The agent-doc CLI (${facts.cliCommand}) could not be queried: ${facts.cliNote ?: "unknown error"}.")
    }
    val cliExpected = cli?.expectedPluginVersion
    if (cliExpected != null && cliExpected != facts.pluginVersion) {
        add(
            "Plugin ${facts.pluginVersion} is not the version agent-doc CLI ${cli?.version ?: "?"} expects " +
                "($cliExpected). Update the plugin or the binary so they come from the same release.",
        )
    }
    val nativeExpected = native?.expectedPluginVersion
    if (nativeExpected != null && nativeExpected != facts.pluginVersion && nativeExpected != cliExpected) {
        add(
            "Plugin ${facts.pluginVersion} is not the version the loaded native library " +
                "${native?.version ?: "?"} expects ($nativeExpected).",
        )
    }
    if (cli != null && native != null) {
        if (cli.buildId != null && native.buildId != null) {
            if (cli.buildId != native.buildId) {
                add(
                    "The agent-doc CLI (build ${cli.buildId}) and the loaded native library " +
                        "(build ${native.buildId}) are different builds; the IPC handshake rejects " +
                        "mismatched builds. Reinstall agent-doc, then reload the plugin or restart the IDE.",
                )
            }
        } else if (cli.version != null && native.version != null && cli.version != native.version) {
            add(
                "The agent-doc CLI is ${cli.version} but the loaded native library is ${native.version}. " +
                    "Reinstall agent-doc, then reload the plugin or restart the IDE.",
            )
        }
    }
}

/** The dialog body: versions first, then warnings. */
internal fun renderAboutReport(facts: AboutAgentDocFacts, mismatches: List<String> = aboutMismatches(facts)): String =
    buildString {
        appendLine("Agent Doc plugin (JetBrains): ${facts.pluginVersion}")
        appendLine()
        appendLine("agent-doc CLI: ${facts.cli?.version ?: "unavailable"}")
        appendLine("  binary: ${facts.cli?.executable ?: facts.cliCommand}")
        appendLine("  build id: ${facts.cli?.buildId ?: "unknown"}")
        facts.cli?.library?.let { appendLine("  paired native library: $it") }
        facts.cli?.expectedPluginVersion?.let { appendLine("  expects plugin: $it") }
        facts.cliNote?.takeIf { facts.cli != null }?.let { appendLine("  note: $it") }
        appendLine()
        appendLine("Native library: ${facts.native?.version ?: "not loaded"}")
        appendLine("  path: ${facts.nativePath ?: "unresolved"}")
        if (facts.native != null) {
            appendLine("  build id: ${facts.native.buildId ?: "unknown"}")
            facts.native.expectedPluginVersion?.let { appendLine("  expects plugin: $it") }
        }
        facts.nativeNote?.let { appendLine("  note: $it") }
        if (mismatches.isEmpty()) {
            appendLine()
            append("Plugin, CLI, and native library agree.")
        } else {
            appendLine()
            appendLine("Version mismatch:")
            mismatches.forEach { appendLine("- $it") }
        }
    }.trimEnd()

/** Native side of the facts, from the generation the plugin already loaded. */
internal fun nativeAboutFacts(snapshot: NativeAboutSnapshot): Pair<AgentDocBuildFacts?, String?> {
    if (snapshot.version == null) {
        return null to (snapshot.loadError ?: "loads when an agent-doc markdown document is opened")
    }
    val parsed = snapshot.buildInfoJson?.let { parseBuildInfoJson(it) }
    val facts =
        AgentDocBuildFacts(
            version = parsed?.version ?: snapshot.version,
            buildId = parsed?.buildId,
            library = snapshot.path,
            expectedPluginVersion = parsed?.expectedPluginVersion,
        )
    val note = if (parsed == null) "library predates agent_doc_build_info_json; build id unavailable" else null
    return facts to note
}

/**
 * `editoractionmenu`: "About Agent Doc" shows which plugin, agent-doc CLI binary, and native
 * library are actually running, and warns when they are not one release. The CLI is the binary
 * every other action runs (`TerminalUtil.resolveAgentDoc`), queried with `agent-doc version
 * --json` (falling back to `--version` for older binaries). The native library is the generation
 * already loaded; About never loads or reloads it.
 */
class AboutAgentDocAction : AnAction(), DumbAware {
    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val projectRoot = TerminalUtil.cleanupProjectRoot(project)
        ApplicationManager.getApplication().executeOnPooledThread {
            val facts = gatherFacts(projectRoot)
            val mismatches = aboutMismatches(facts)
            val report = renderAboutReport(facts, mismatches)
            LOG.info("[about] ${report.replace("\n", " | ")}")
            ApplicationManager.getApplication().invokeLater {
                show(project, report, mismatches.isNotEmpty())
            }
        }
    }

    private fun show(project: Project, report: String, warn: Boolean) {
        val choice =
            Messages.showDialog(
                project,
                report,
                "About Agent Doc",
                arrayOf("Copy", "Close"),
                1,
                if (warn) Messages.getWarningIcon() else Messages.getInformationIcon(),
            )
        if (choice == 0) {
            CopyPasteManager.getInstance().setContents(StringSelection(report))
        }
    }

    private fun gatherFacts(projectRoot: String): AboutAgentDocFacts {
        val agentDoc = TerminalUtil.resolveAgentDoc(projectRoot)
        val (cli, cliNote) = queryCli(agentDoc, projectRoot)
        val snapshot = AgentDocLib.aboutSnapshot()
        val (native, nativeNote) = nativeAboutFacts(snapshot)
        return AboutAgentDocFacts(
            pluginVersion = pluginVersion(),
            cliCommand = agentDoc,
            cli = cli,
            cliNote = cliNote,
            nativePath = snapshot.path,
            native = native,
            nativeNote = nativeNote,
        )
    }

    private fun queryCli(agentDoc: String, projectRoot: String): Pair<AgentDocBuildFacts?, String?> =
        try {
            val json =
                SyncLayoutAction.runCommandWithTimeout(
                    listOf(agentDoc, "version", "--json"),
                    projectRoot,
                    ABOUT_CLI_TIMEOUT_MS,
                    captureStderrSeparately = true,
                )
            val parsed = if (json.exitCode == 0) parseBuildInfoJson(json.output) else null
            if (parsed != null) {
                parsed.copy(executable = parsed.executable ?: agentDoc) to null
            } else {
                val text =
                    SyncLayoutAction.runCommandWithTimeout(
                        listOf(agentDoc, "--version"),
                        projectRoot,
                        ABOUT_CLI_TIMEOUT_MS,
                        captureStderrSeparately = true,
                    )
                val version = if (text.exitCode == 0) parseAgentDocVersionText(text.output) else null
                if (version != null) {
                    AgentDocBuildFacts(version = version, executable = agentDoc) to
                        "binary predates `agent-doc version --json`; build id unavailable"
                } else {
                    null to
                        (text.errorOutput.ifBlank { json.errorOutput }.ifBlank { "exit ${text.exitCode}" })
                }
            }
        } catch (error: Exception) {
            null to (error.message ?: error.javaClass.simpleName)
        }

    override fun update(e: AnActionEvent) {
        e.presentation.isEnabledAndVisible = e.project != null
    }

    override fun getActionUpdateThread(): ActionUpdateThread = ActionUpdateThread.BGT

    private companion object {
        private val LOG = Logger.getInstance(AboutAgentDocAction::class.java)
    }
}
