package com.github.btakita.agentdoc

import com.github.btakita.agentdoc.shell.ComponentFold
import com.intellij.openapi.editor.Document
import com.intellij.psi.PsiElement

/**
 * GH #19 phase 5: fold each `<!-- agent:NAME -->` … `<!-- /agent:NAME -->` component of a
 * session document to `<!-- agent:NAME · N items -->`. The shell's `ShellComponentFoldingBuilder`
 * (GH #339 R1) builds the fold descriptors from these regions.
 *
 * Registered for Markdown only (`agent-doc-markdown.xml`); a document without an installed
 * native outline (any non-session Markdown file) yields no regions. Per pass this reads the
 * cached range markers and scans component bodies for the item count; it never calls native code.
 */
internal object AgentDocComponentFoldingBuilder {
    fun foldRegions(root: PsiElement, document: Document): List<ComponentFold> {
        val file = root.containingFile ?: return emptyList()
        if (root != file) return emptyList()
        val spans = ComponentOutlineStore.spansFor(file.project, document)
        if (spans.isEmpty()) return emptyList()
        if (file.node == null) return emptyList()
        val length = file.textLength
        return AgentDocComponentOutline.foldSpecs(document.charsSequence, spans)
            .filter { it.end <= length }
            .map { spec -> ComponentFold(spec.start, spec.end, spec.placeholder) }
    }
}
