package com.github.btakita.agentdoc

import com.intellij.icons.AllIcons
import com.intellij.ide.structureView.StructureViewExtension
import com.intellij.ide.structureView.StructureViewTreeElement
import com.intellij.navigation.ItemPresentation
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.fileEditor.OpenFileDescriptor
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import javax.swing.Icon

/**
 * GH #19 phase 7: add a session document's components to the Markdown structure view, each with
 * its items (headings, or top-level list items) as children and nested components below it.
 *
 * Contributed as a [StructureViewExtension] on the file root so the Markdown plugin's own heading
 * outline stays intact. The type is [PsiFile] (the Markdown PSI class is not on the compile
 * classpath); [getChildren] returns nothing for any file without an installed native outline,
 * which is every non-session file, after a name check.
 */
class AgentDocComponentStructureViewExtension : StructureViewExtension {
    override fun getType(): Class<out PsiElement> = PsiFile::class.java

    override fun getChildren(parent: PsiElement): Array<StructureViewTreeElement> {
        val file = parent as? PsiFile ?: return EMPTY
        if (!file.name.endsWith(".md")) return EMPTY
        val vFile = file.virtualFile ?: return EMPTY
        val document = file.viewProvider.document ?: return EMPTY
        val spans = ComponentOutlineStore.spansFor(file.project, document)
        if (spans.isEmpty()) return EMPTY
        return AgentDocComponentOutline.structure(document.charsSequence, spans)
            .map { ComponentElement(file.project, vFile, it, it.span.name) }
            .toTypedArray()
    }

    override fun getCurrentEditorElement(editor: Editor, parent: PsiElement): Any? = null

    /** Navigable structure node at [offset] of [file]. */
    private abstract class OffsetElement(
        val project: Project,
        val file: VirtualFile,
        val offset: Int,
    ) : StructureViewTreeElement, ItemPresentation {
        override fun getPresentation(): ItemPresentation = this
        override fun navigate(requestFocus: Boolean) {
            OpenFileDescriptor(project, file, offset).navigate(requestFocus)
        }
        override fun canNavigate(): Boolean = file.isValid
        override fun canNavigateToSource(): Boolean = file.isValid
    }

    private class ComponentElement(
        project: Project,
        file: VirtualFile,
        val node: AgentDocComponentOutline.ComponentNode,
        /** Nesting path (`exchange/inner`): a key that survives edits shifting offsets. */
        val path: String,
    ) : OffsetElement(project, file, node.span.openStart) {
        override fun getValue(): Any = ComponentKey(file.url, path)
        override fun getPresentableText(): String = node.presentableText
        override fun getLocationString(): String = node.locationString
        override fun getIcon(unused: Boolean): Icon = AllIcons.Nodes.Template
        override fun getChildren(): Array<StructureViewTreeElement> =
            (
                node.children.map { ComponentElement(project, file, it, path + "/" + it.span.name) } +
                    node.items.mapIndexed { index, item ->
                        val occurrence = node.items.subList(0, index).count { it.label == item.label }
                        ItemElement(project, file, ItemKey(file.url, path, item.label, occurrence), item)
                    }
                ).sortedBy { (it as OffsetElement).offset }.toTypedArray()

        override fun equals(other: Any?): Boolean = other is ComponentElement && other.value == value
        override fun hashCode(): Int = value.hashCode()
    }

    private class ItemElement(
        project: Project,
        file: VirtualFile,
        val key: ItemKey,
        val item: AgentDocComponentOutline.ComponentItem,
    ) : OffsetElement(project, file, item.offset) {
        override fun getValue(): Any = key
        override fun getPresentableText(): String = item.label
        override fun getLocationString(): String? = null
        override fun getIcon(unused: Boolean): Icon =
            when (item.kind) {
                AgentDocComponentOutline.ItemKind.HEADING -> AllIcons.Nodes.Folder
                AgentDocComponentOutline.ItemKind.LIST_ITEM -> AllIcons.Nodes.Property
            }
        override fun getChildren(): Array<StructureViewTreeElement> = EMPTY

        override fun equals(other: Any?): Boolean = other is ItemElement && other.value == value
        override fun hashCode(): Int = value.hashCode()
    }

    private data class ComponentKey(val url: String, val path: String)

    private data class ItemKey(val url: String, val path: String, val label: String, val occurrence: Int)

    private companion object {
        val EMPTY: Array<StructureViewTreeElement> = emptyArray()
    }
}
