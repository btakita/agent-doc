package com.github.btakita.agentdoc

import com.github.btakita.agentdoc.shell.StructureNode
import com.github.btakita.agentdoc.shell.StructureNodeKind
import com.intellij.psi.PsiFile

/**
 * GH #19 phase 7: a session document's components in the Markdown structure view, each with its
 * items (headings, or top-level list items) as children and nested components below it. The
 * shell's `ShellComponentStructureViewExtension` (GH #339 R1) builds the tree elements from these
 * nodes.
 *
 * Contributed as a structure-view extension on the file root, so the Markdown plugin's own
 * heading outline stays intact. The type is `PsiFile` (the Markdown PSI class is not on the
 * compile classpath); [nodes] returns nothing for any file without an installed native outline,
 * which is every non-session file, after a name check.
 */
internal object AgentDocComponentStructureViewExtension {
    fun nodes(file: PsiFile): List<StructureNode> {
        if (!file.name.endsWith(".md")) return emptyList()
        val vFile = file.virtualFile ?: return emptyList()
        val document = file.viewProvider.document ?: return emptyList()
        val spans = ComponentOutlineStore.spansFor(file.project, document)
        if (spans.isEmpty()) return emptyList()
        return AgentDocComponentOutline.structure(document.charsSequence, spans)
            .map { componentNode(vFile.url, it, it.span.name) }
    }

    /**
     * [path] is the nesting path (`exchange/inner`): with the file URL it is a key that survives
     * edits shifting offsets.
     */
    private fun componentNode(url: String, node: AgentDocComponentOutline.ComponentNode, path: String): StructureNode {
        val children =
            node.children.map { componentNode(url, it, path + "/" + it.span.name) } +
                node.items.mapIndexed { index, item ->
                    val occurrence = node.items.subList(0, index).count { it.label == item.label }
                    StructureNode(
                        key = key("item", url, path, item.label, occurrence.toString()),
                        presentableText = item.label,
                        locationString = null,
                        kind = when (item.kind) {
                            AgentDocComponentOutline.ItemKind.HEADING -> StructureNodeKind.HEADING
                            AgentDocComponentOutline.ItemKind.LIST_ITEM -> StructureNodeKind.LIST_ITEM
                        },
                        offset = item.offset,
                        children = emptyList(),
                    )
                }
        return StructureNode(
            key = key("component", url, path),
            presentableText = node.presentableText,
            locationString = node.locationString,
            kind = StructureNodeKind.COMPONENT,
            offset = node.span.openStart,
            children = children.sortedBy { it.offset },
        )
    }

    /** NUL-separated: no URL, component path or item label contains one. */
    private fun key(vararg parts: String): String = parts.joinToString("\u0000")
}
