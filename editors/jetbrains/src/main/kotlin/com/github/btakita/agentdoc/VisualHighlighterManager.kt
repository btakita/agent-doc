package com.github.btakita.agentdoc

import com.intellij.openapi.Disposable
import com.intellij.openapi.editor.Document
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.editor.DefaultLanguageHighlighterColors
import com.intellij.openapi.editor.EditorFactory
import com.intellij.openapi.editor.event.DocumentEvent
import com.intellij.openapi.editor.event.DocumentListener
import com.intellij.openapi.editor.event.EditorFactoryEvent
import com.intellij.openapi.editor.event.EditorFactoryListener
import com.intellij.openapi.editor.markup.EffectType
import com.intellij.openapi.editor.markup.HighlighterLayer
import com.intellij.openapi.editor.markup.HighlighterTargetArea
import com.intellij.openapi.editor.markup.TextAttributes
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.Key
import com.intellij.util.Alarm
import java.awt.Font
import java.util.concurrent.ConcurrentHashMap

class VisualHighlighterManager private constructor(private val project: Project) : Disposable {
    private val alarm = Alarm(Alarm.ThreadToUse.SWING_THREAD, this)

    init {
        EditorFactory.getInstance().eventMulticaster.addDocumentListener(object : DocumentListener {
            override fun documentChanged(event: DocumentEvent) {
                scheduleRefresh(event.document)
            }
        }, this)
        EditorFactory.getInstance().addEditorFactoryListener(object : EditorFactoryListener {
            override fun editorCreated(event: EditorFactoryEvent) {
                refreshEditor(event.editor)
            }
        }, this)
        refreshAll()
    }

    private fun scheduleRefresh(document: Document) {
        if (!isMarkdown(document)) return
        alarm.cancelAllRequests()
        alarm.addRequest({ refreshDocument(document) }, 120)
    }

    private fun refreshAll() {
        EditorFactory.getInstance().allEditors
            .filter { it.project == project }
            .forEach { refreshEditor(it) }
    }

    private fun refreshDocument(document: Document) {
        EditorFactory.getInstance().getEditors(document, project).forEach { refreshEditor(it) }
    }

    private fun refreshEditor(editor: Editor) {
        if (editor.isDisposed) return
        if (!isMarkdown(editor.document)) {
            clearEditor(editor)
            return
        }

        clearEditor(editor)
        val tokens = NativePatching.visualTokens(editor.document.text)
        val markup = editor.markupModel
        for (token in tokens) {
            if (token.end <= token.start || token.end > editor.document.textLength) continue
            val highlighter = markup.addRangeHighlighter(
                token.start,
                token.end,
                HighlighterLayer.ADDITIONAL_SYNTAX,
                attrsFor(editor, token.kind),
                HighlighterTargetArea.EXACT_RANGE,
            )
            highlighter.putUserData(HIGHLIGHTER_KEY, true)
        }
    }

    private fun clearEditor(editor: Editor) {
        val markup = editor.markupModel
        markup.allHighlighters
            .filter { it.getUserData(HIGHLIGHTER_KEY) == true }
            .forEach { markup.removeHighlighter(it) }
    }

    private fun isMarkdown(document: Document): Boolean {
        val file = FileDocumentManager.getInstance().getFile(document) ?: return false
        return file.name.endsWith(".md")
    }

    private fun attrsFor(editor: Editor, kind: String): TextAttributes {
        fun baseAttrs(base: TextAttributes?): TextAttributes {
            return TextAttributes(
                base?.foregroundColor,
                base?.backgroundColor,
                base?.effectColor,
                base?.effectType,
                base?.fontType ?: Font.PLAIN,
            )
        }

        return when (kind) {
            "component_open", "component_close" -> baseAttrs(
                editor.colorsScheme.getAttributes(DefaultLanguageHighlighterColors.METADATA)
            ).apply {
                fontType = Font.BOLD
            }
            "patch_open", "patch_close" -> baseAttrs(
                editor.colorsScheme.getAttributes(DefaultLanguageHighlighterColors.MARKUP_ATTRIBUTE)
            ).apply {
                fontType = Font.BOLD
            }
            "boundary" -> baseAttrs(
                editor.colorsScheme.getAttributes(DefaultLanguageHighlighterColors.KEYWORD)
            ).apply {
                fontType = Font.ITALIC
            }
            "scratch_comment" -> baseAttrs(
                editor.colorsScheme.getAttributes(DefaultLanguageHighlighterColors.BLOCK_COMMENT)
            ).apply {
                fontType = Font.ITALIC
            }
            "prompt" -> baseAttrs(
                editor.colorsScheme.getAttributes(DefaultLanguageHighlighterColors.STRING)
            ).apply {
                fontType = Font.BOLD
            }
            "response_heading" -> baseAttrs(
                editor.colorsScheme.getAttributes(DefaultLanguageHighlighterColors.FUNCTION_DECLARATION)
            ).apply {
                fontType = Font.BOLD
            }
            "tracked_id" -> baseAttrs(
                editor.colorsScheme.getAttributes(DefaultLanguageHighlighterColors.CONSTANT)
            ).apply {
                fontType = Font.BOLD
                effectType = EffectType.ROUNDED_BOX
                effectColor = foregroundColor
            }
            else -> TextAttributes()
        }
    }

    override fun dispose() {
        EditorFactory.getInstance().allEditors
            .filter { it.project == project }
            .forEach { clearEditor(it) }
    }

    companion object {
        private val HIGHLIGHTER_KEY = Key.create<Boolean>("agent-doc.visual.highlighter")
        private val INSTANCES = ConcurrentHashMap<Project, VisualHighlighterManager>()

        fun getInstance(project: Project): VisualHighlighterManager {
            return INSTANCES.computeIfAbsent(project) { VisualHighlighterManager(it) }
        }

        fun disposeProject(project: Project) {
            INSTANCES.remove(project)?.dispose()
        }
    }
}
