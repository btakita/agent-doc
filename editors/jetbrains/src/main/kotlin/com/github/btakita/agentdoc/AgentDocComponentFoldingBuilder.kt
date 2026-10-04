package com.github.btakita.agentdoc

import com.intellij.lang.ASTNode
import com.intellij.lang.folding.FoldingBuilderEx
import com.intellij.lang.folding.FoldingDescriptor
import com.intellij.openapi.editor.Document
import com.intellij.openapi.project.DumbAware
import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiElement

/**
 * GH #19 phase 5: fold each `<!-- agent:NAME -->` … `<!-- /agent:NAME -->` component of a
 * session document to `<!-- agent:NAME · N items -->`.
 *
 * Registered for Markdown only (`agent-doc-markdown.xml`); a document without an installed
 * native outline (any non-session Markdown file) yields no regions. Per pass this reads the
 * cached range markers and scans component bodies for the item count; it never calls native code.
 */
class AgentDocComponentFoldingBuilder : FoldingBuilderEx(), DumbAware {
    override fun buildFoldRegions(root: PsiElement, document: Document, quick: Boolean): Array<FoldingDescriptor> {
        val file = root.containingFile ?: return FoldingDescriptor.EMPTY_ARRAY
        if (root != file) return FoldingDescriptor.EMPTY_ARRAY
        val spans = ComponentOutlineStore.spansFor(file.project, document)
        if (spans.isEmpty()) return FoldingDescriptor.EMPTY_ARRAY
        val node = file.node ?: return FoldingDescriptor.EMPTY_ARRAY
        val length = file.textLength
        return AgentDocComponentOutline.foldSpecs(document.charsSequence, spans)
            .filter { it.end <= length }
            .map { spec -> FoldingDescriptor(node, TextRange(spec.start, spec.end), null, spec.placeholder) }
            .toTypedArray()
    }

    override fun getPlaceholderText(node: ASTNode): String = "<!-- agent:… -->"

    override fun isCollapsedByDefault(node: ASTNode): Boolean = false
}
