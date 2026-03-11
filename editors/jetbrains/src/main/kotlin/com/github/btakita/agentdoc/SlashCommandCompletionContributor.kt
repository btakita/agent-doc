package com.github.btakita.agentdoc

import com.intellij.codeInsight.completion.*
import com.intellij.codeInsight.lookup.LookupElementBuilder
import com.intellij.openapi.diagnostic.Logger
import com.intellij.patterns.PlatformPatterns
import com.intellij.util.ProcessingContext
import com.google.gson.Gson
import com.google.gson.reflect.TypeToken

/**
 * Provides autocomplete for Claude Code slash commands in markdown files.
 *
 * Triggers when the user types `/` at the start of a line.
 * Commands are loaded from `agent-doc commands` CLI output on first use.
 */
class SlashCommandCompletionContributor : CompletionContributor() {

    private val log = Logger.getInstance(SlashCommandCompletionContributor::class.java)

    init {
        extend(
            CompletionType.BASIC,
            PlatformPatterns.psiElement(),
            object : CompletionProvider<CompletionParameters>() {
                override fun addCompletions(
                    parameters: CompletionParameters,
                    context: ProcessingContext,
                    result: CompletionResultSet
                ) {
                    val document = parameters.editor.document
                    val offset = parameters.offset
                    val lineNumber = document.getLineNumber(offset)
                    val lineStart = document.getLineStartOffset(lineNumber)
                    val textBeforeCaret = document.getText(com.intellij.openapi.util.TextRange(lineStart, offset))

                    // Only trigger on lines starting with `/`
                    val trimmed = textBeforeCaret.trimStart()
                    if (!trimmed.startsWith("/")) return

                    val commands = getCommands(parameters.editor.project ?: return)
                    val prefix = trimmed

                    for (cmd in commands) {
                        if (!cmd.name.startsWith(prefix.substringBefore(" ").ifEmpty { "/" })) continue
                        val element = LookupElementBuilder.create(cmd.name)
                            .withTailText(" ${cmd.args}", true)
                            .withTypeText(cmd.description, true)
                            .withBoldness(cmd.name.count { it == ' ' } == 0) // Bold top-level commands
                        result.addElement(element)
                    }
                }
            }
        )
    }

    data class CommandInfo(val name: String, val args: String, val description: String)

    companion object {
        @Volatile
        private var cachedCommands: List<CommandInfo>? = null

        fun getCommands(project: com.intellij.openapi.project.Project): List<CommandInfo> {
            cachedCommands?.let { return it }

            val agentDoc = try { TerminalUtil.resolveAgentDoc() } catch (_: Exception) { return emptyList() }

            return try {
                val process = ProcessBuilder(agentDoc, "commands")
                    .directory(project.basePath?.let { java.io.File(it) })
                    .redirectErrorStream(false)
                    .start()
                val output = process.inputStream.bufferedReader().readText()
                val exitCode = process.waitFor()
                if (exitCode != 0) return emptyList()

                val type = object : TypeToken<List<CommandInfo>>() {}.type
                val commands: List<CommandInfo> = Gson().fromJson(output, type)
                cachedCommands = commands
                commands
            } catch (e: Exception) {
                Logger.getInstance(SlashCommandCompletionContributor::class.java)
                    .warn("Failed to load commands from agent-doc CLI", e)
                emptyList()
            }
        }
    }
}
