package com.github.btakita.agentdoc

import com.intellij.codeInsight.completion.*
import com.intellij.codeInsight.lookup.LookupElementBuilder
import com.intellij.openapi.diagnostic.Logger
import com.intellij.patterns.PlatformPatterns
import com.intellij.util.ProcessingContext

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
                    val file = parameters.originalFile.virtualFile ?: return
                    // Only activate in markdown files
                    if (!file.name.endsWith(".md")) return

                    val document = parameters.editor.document
                    val offset = parameters.offset
                    val lineNumber = document.getLineNumber(offset)
                    val lineStart = document.getLineStartOffset(lineNumber)
                    val textBeforeCaret = document.getText(
                        com.intellij.openapi.util.TextRange(lineStart, offset)
                    )

                    // Only trigger on lines starting with `/`
                    val trimmed = textBeforeCaret.trimStart()
                    if (!trimmed.startsWith("/")) return

                    log.info("Slash completion triggered: trimmed='$trimmed', file=${file.name}")

                    val commands = getCommands(parameters.editor.project ?: return)
                    log.info("Got ${commands.size} commands")
                    if (commands.isEmpty()) return

                    // Use the full slash-prefixed text as the prefix matcher
                    // so IntelliJ correctly filters our results
                    val slashResult = result.withPrefixMatcher(
                        PlainPrefixMatcher(trimmed)
                    )

                    for (cmd in commands) {
                        val element = LookupElementBuilder.create(cmd.name)
                            .withTailText(" ${cmd.args}", true)
                            .withTypeText(cmd.description, true)
                            .withBoldness(cmd.name.count { it == ' ' } == 0)
                        slashResult.addElement(element)
                    }
                }
            }
        )
    }

    data class CommandInfo(val name: String, val args: String, val description: String)

    companion object {
        private val log = Logger.getInstance(SlashCommandCompletionContributor::class.java)

        @Volatile
        private var cachedCommands: List<CommandInfo>? = null

        fun getCommands(project: com.intellij.openapi.project.Project): List<CommandInfo> {
            cachedCommands?.let { return it }

            val agentDoc = try {
                TerminalUtil.resolveAgentDoc()
            } catch (e: Exception) {
                log.warn("Failed to resolve agent-doc binary", e)
                return emptyList()
            }

            log.info("Resolved agent-doc at: $agentDoc")

            return try {
                val process = ProcessBuilder(agentDoc, "commands")
                    .directory(project.basePath?.let { java.io.File(it) })
                    .redirectErrorStream(false)
                    .start()
                val output = process.inputStream.bufferedReader().readText()
                val stderr = process.errorStream.bufferedReader().readText()
                val exitCode = process.waitFor()

                if (exitCode != 0) {
                    log.warn("agent-doc commands failed (exit $exitCode): $stderr")
                    return emptyList()
                }

                log.info("agent-doc commands output length: ${output.length}")

                val commands = parseCommandsJson(output)
                log.info("Parsed ${commands.size} commands")
                cachedCommands = commands
                commands
            } catch (e: Exception) {
                log.warn("Failed to load commands from agent-doc CLI", e)
                emptyList()
            }
        }

        /**
         * Parse JSON array of commands without requiring Gson.
         * Format: [{"name":"/foo","args":"<bar>","description":"..."},...]
         */
        fun parseCommandsJson(json: String): List<CommandInfo> {
            val result = mutableListOf<CommandInfo>()
            // Simple state-machine JSON array-of-objects parser
            var i = 0
            val len = json.length

            fun skipWhitespace() { while (i < len && json[i].isWhitespace()) i++ }

            fun readString(): String {
                if (i >= len || json[i] != '"') throw IllegalStateException("Expected '\"' at $i")
                i++ // skip opening quote
                val sb = StringBuilder()
                while (i < len && json[i] != '"') {
                    if (json[i] == '\\' && i + 1 < len) {
                        i++
                        when (json[i]) {
                            '"' -> sb.append('"')
                            '\\' -> sb.append('\\')
                            'n' -> sb.append('\n')
                            't' -> sb.append('\t')
                            '/' -> sb.append('/')
                            else -> { sb.append('\\'); sb.append(json[i]) }
                        }
                    } else {
                        sb.append(json[i])
                    }
                    i++
                }
                if (i < len) i++ // skip closing quote
                return sb.toString()
            }

            skipWhitespace()
            if (i >= len || json[i] != '[') return result
            i++ // skip [

            while (i < len) {
                skipWhitespace()
                if (i >= len || json[i] == ']') break

                if (json[i] == ',') { i++; continue }

                if (json[i] != '{') break
                i++ // skip {

                var name = ""
                var args = ""
                var description = ""

                while (i < len && json[i] != '}') {
                    skipWhitespace()
                    if (json[i] == ',') { i++; continue }
                    if (json[i] == '}') break

                    val key = readString()
                    skipWhitespace()
                    if (i < len && json[i] == ':') i++ // skip :
                    skipWhitespace()
                    val value = readString()

                    when (key) {
                        "name" -> name = value
                        "args" -> args = value
                        "description" -> description = value
                    }
                }
                if (i < len && json[i] == '}') i++ // skip }

                if (name.isNotEmpty()) {
                    result.add(CommandInfo(name, args, description))
                }
            }

            return result
        }
    }
}
