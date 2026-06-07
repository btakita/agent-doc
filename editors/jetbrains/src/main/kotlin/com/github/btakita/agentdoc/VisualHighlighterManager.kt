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
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.openapi.vfs.VirtualFileEvent
import com.intellij.openapi.vfs.VirtualFileListener
import com.intellij.psi.PsiManager
import com.intellij.util.Alarm
import java.awt.Color
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
        com.intellij.openapi.vfs.VirtualFileManager.getInstance().addVirtualFileListener(
            object : VirtualFileListener {
                override fun contentsChanged(event: VirtualFileEvent) {
                    if (!event.file.name.endsWith(".md")) return
                    reparseAndRefresh(event.file)
                }
            },
            this,
        )
        refreshAll()
    }

    private fun reparseAndRefresh(file: VirtualFile) {
        val document = FileDocumentManager.getInstance().getDocument(file) ?: return
        try {
            PsiManager.getInstance(project).dropPsiCaches()
        } catch (_: Exception) {
            // Best-effort PSI cache drop; the VisualHighlighterManager still refreshes via documentChanged
        }
        scheduleRefresh(document)
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

    fun refreshFile(file: VirtualFile) {
        val document = FileDocumentManager.getInstance().getDocument(file) ?: return
        scheduleRefresh(document)
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
                layerFor(token.kind),
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
            "component_body" -> TextAttributes().apply {
                backgroundColor = mutedBackground(
                    editor,
                    editor.colorsScheme.getAttributes(DefaultLanguageHighlighterColors.METADATA)?.foregroundColor
                )
            }
            "scratch_comment_body" -> baseAttrs(
                editor.colorsScheme.getAttributes(DefaultLanguageHighlighterColors.BLOCK_COMMENT)
            ).apply {
                backgroundColor = mutedBackground(editor, foregroundColor)
                fontType = Font.ITALIC
            }
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
            // #editor-bold-markdown-rendering: render markdown emphasis inline.
            "bold" -> baseAttrs(editor.colorsScheme.getAttributes(DefaultLanguageHighlighterColors.IDENTIFIER))
                .apply { fontType = Font.BOLD }
            "italic" -> baseAttrs(editor.colorsScheme.getAttributes(DefaultLanguageHighlighterColors.IDENTIFIER))
                .apply { fontType = Font.ITALIC }
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
                backgroundColor = mutedBackground(editor, foregroundColor)
            }
            "label_tag" -> baseAttrs(
                editor.colorsScheme.getAttributes(DefaultLanguageHighlighterColors.MARKUP_ATTRIBUTE)
            ).apply {
                fontType = Font.BOLD
                effectType = EffectType.ROUNDED_BOX
                effectColor = foregroundColor
                backgroundColor = mutedBackground(editor, foregroundColor)
            }
            else -> TextAttributes()
        }
    }

    private fun layerFor(kind: String): Int =
        when (kind) {
            "component_body", "scratch_comment_body" -> HighlighterLayer.ADDITIONAL_SYNTAX - 1
            else -> HighlighterLayer.ADDITIONAL_SYNTAX
        }

    private fun mutedBackground(editor: Editor, accent: Color?): Color {
        val base = editor.colorsScheme.defaultBackground
        return blend(base, accent ?: base, 0.10f)
    }

    private fun blend(base: Color, accent: Color, accentRatio: Float): Color {
        val clamped = accentRatio.coerceIn(0f, 1f)
        val baseRatio = 1f - clamped
        return Color(
            (base.red * baseRatio + accent.red * clamped).toInt().coerceIn(0, 255),
            (base.green * baseRatio + accent.green * clamped).toInt().coerceIn(0, 255),
            (base.blue * baseRatio + accent.blue * clamped).toInt().coerceIn(0, 255),
        )
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
