package com.github.btakita.agentdoc

import com.intellij.codeInsight.daemon.GutterIconNavigationHandler
import com.intellij.codeInsight.daemon.LineMarkerInfo
import com.intellij.codeInsight.daemon.LineMarkerProvider
import com.intellij.icons.AllIcons
import com.intellij.ide.DataManager
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.application.ReadAction
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.editor.markup.GutterIconRenderer
import com.intellij.openapi.project.DumbAware
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiElement
import java.awt.event.MouseEvent

/**
 * GH #19 phase 6: a gutter icon on each component open marker of a session document. The
 * tooltip names the component, its item count and inline attributes; clicking toggles the
 * component's fold region.
 *
 * Markers attach to leaf elements only, and only to the single leaf whose range contains an open
 * marker's first offset, so each component gets exactly one icon whatever the Markdown PSI shape.
 */
class AgentDocComponentLineMarkerProvider : LineMarkerProvider, DumbAware {
    override fun getLineMarkerInfo(element: PsiElement): LineMarkerInfo<*>? {
        if (element.firstChild != null) return null
        val file = element.containingFile ?: return null
        val document = file.viewProvider.document ?: return null
        val spans = ComponentOutlineStore.spansFor(file.project, document)
        if (spans.isEmpty()) return null
        val range = element.textRange ?: return null
        val span = AgentDocComponentOutline.componentOpeningIn(spans, range.startOffset, range.endOffset)
            ?: return null
        val name = span.name
        return LineMarkerInfo(
            element,
            range,
            AllIcons.Nodes.Template,
            { e -> tooltipFor(e, name) },
            ToggleComponentFold,
            GutterIconRenderer.Alignment.LEFT,
            { "agent:$name component" },
        )
    }

    private fun tooltipFor(element: PsiElement, name: String): String =
        ReadAction.compute<String, RuntimeException> { tooltipInRead(element, name) }

    private fun tooltipInRead(element: PsiElement, name: String): String {
        if (!element.isValid) return "agent:$name"
        val file = element.containingFile ?: return "agent:$name"
        val document = file.viewProvider.document ?: return "agent:$name"
        val spans = ComponentOutlineStore.spansFor(file.project, document)
        val range = element.textRange ?: return "agent:$name"
        val span = AgentDocComponentOutline.componentOpeningIn(spans, range.startOffset, range.endOffset)
            ?: return "agent:$name"
        return AgentDocComponentOutline.tooltip(document.charsSequence, span, spans)
    }

    private object ToggleComponentFold : GutterIconNavigationHandler<PsiElement> {
        override fun navigate(e: MouseEvent, elt: PsiElement) {
            val editor = DataManager.getInstance().getDataContext(e.component)
                .getData(CommonDataKeys.EDITOR) ?: return
            toggle(editor, elt)
        }
    }

    companion object {
        /** Toggle the fold of the component whose open marker [elt] holds; no-op without one. */
        internal fun toggle(editor: Editor, elt: PsiElement) {
            val project = editor.project ?: return
            val document = editor.document
            val spec = ReadAction.compute<AgentDocComponentOutline.FoldSpec?, RuntimeException> {
                if (!elt.isValid) return@compute null
                if (PsiDocumentManager.getInstance(project).getDocument(elt.containingFile) != document) {
                    return@compute null
                }
                val spans = ComponentOutlineStore.spansFor(project, document)
                val range = elt.textRange ?: return@compute null
                val span = AgentDocComponentOutline.componentOpeningIn(spans, range.startOffset, range.endOffset)
                    ?: return@compute null
                AgentDocComponentOutline.foldSpecs(document.charsSequence, listOf(span)).firstOrNull()
            } ?: return
            val folding = editor.foldingModel
            val region = folding.getFoldRegion(spec.start, spec.end) ?: return
            folding.runBatchFoldingOperation { region.isExpanded = !region.isExpanded }
        }
    }
}
