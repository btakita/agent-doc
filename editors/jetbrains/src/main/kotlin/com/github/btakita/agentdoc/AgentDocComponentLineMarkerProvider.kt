package com.github.btakita.agentdoc

import com.intellij.openapi.application.ReadAction
import com.intellij.openapi.editor.Editor
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiElement

/**
 * GH #19 phase 6: a gutter icon on each component open marker of a session document. The
 * tooltip names the component, its item count and inline attributes; clicking toggles the
 * component's fold region. The shell's `ShellComponentLineMarkerProvider` (GH #339 R1) builds
 * the marker, its tooltip provider and its click handler around these functions.
 *
 * Markers attach to leaf elements only, and only to the single leaf whose range contains an open
 * marker's first offset, so each component gets exactly one icon whatever the Markdown PSI shape.
 */
internal object AgentDocComponentLineMarkerProvider {
    /** The component whose open marker leaf [element] holds, or null for any other element. */
    fun markerName(element: PsiElement): String? {
        if (element.firstChild != null) return null
        val file = element.containingFile ?: return null
        val document = file.viewProvider.document ?: return null
        val spans = ComponentOutlineStore.spansFor(file.project, document)
        if (spans.isEmpty()) return null
        val range = element.textRange ?: return null
        return AgentDocComponentOutline.componentOpeningIn(spans, range.startOffset, range.endOffset)?.name
    }

    fun tooltipFor(element: PsiElement, name: String): String =
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

    /** Toggle the fold of the component whose open marker [elt] holds; no-op without one. */
    fun toggle(editor: Editor, elt: PsiElement) {
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
