package com.github.btakita.agentdoc.shell

import com.intellij.codeInsight.completion.CompletionParameters
import com.intellij.openapi.Disposable
import com.intellij.openapi.editor.Document
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile

/*
 * GH #339 R1, editor and language extensions (editors/jetbrains/SPEC.md "Who owns IntelliJ
 * registrations and callbacks"): every XML extension class is a shell forwarder, and the
 * implementation answers it with plain data. The shell builds whatever the platform keeps (lookup
 * elements, fold descriptors, line markers, structure-view nodes, the status-bar widget and the
 * editor banner), so no platform-held object is an implementation class.
 */

/** What an implementation generation answers the shell's editor and language extensions with. */
interface ImplEditorExtensions {
    /** Slash-command completion at [parameters]' caret, or null when none applies. */
    fun slashCompletions(parameters: CompletionParameters): SlashCompletions?

    /** Component fold regions for a whole-file folding pass over [root]. */
    fun componentFolds(root: PsiElement, document: Document): List<ComponentFold>

    /** The component name whose open-marker gutter icon belongs on leaf [element], or null. */
    fun componentMarker(element: PsiElement): String?

    /** The gutter tooltip for component [name] at [element]; takes its own read action. */
    fun componentTooltip(element: PsiElement, name: String): String

    /** Toggle the fold of the component whose open marker [element] holds. */
    fun toggleComponentFold(editor: Editor, element: PsiElement)

    /** The session-component structure-view children of [file]; empty for a non-session file. */
    fun componentStructure(file: PsiFile): List<StructureNode>

    /** What the turn-state status-bar widget shows now, from the cached projection. */
    fun turnStateStatus(project: Project): TurnStateStatus

    /** Call [onChange] on each turn-state cache change until [parent] is disposed. */
    fun watchTurnState(project: Project, parent: Disposable, onChange: Runnable)

    /** Ask the turn-state refresher to refresh the selected document. */
    fun requestTurnStateRefresh(project: Project, reason: String)

    /** The turn-state banner [file] shows now, or null for none. */
    fun turnStateBanner(project: Project, file: VirtualFile): TurnStateBanner?
}

/** Lookup data for one slash-completion pass; the shell builds the lookup elements. */
data class SlashCompletions(val prefix: String, val items: List<SlashCompletionItem>)

data class SlashCompletionItem(
    val name: String,
    val tailText: String,
    val typeText: String,
    val bold: Boolean,
)

/** One fold region in document offsets, with its placeholder text. */
data class ComponentFold(val start: Int, val end: Int, val placeholder: String)

/** The icon a structure node shows; the shell maps it to a platform icon. */
enum class StructureNodeKind { COMPONENT, HEADING, LIST_ITEM }

/**
 * One structure-view node. [key] is the node's value: it identifies the node across edits that
 * shift offsets, so the structure view keeps its expansion and selection.
 */
data class StructureNode(
    val key: String,
    val presentableText: String,
    val locationString: String?,
    val kind: StructureNodeKind,
    val offset: Int,
    val children: List<StructureNode>,
)

/** The status-bar text and tooltip; [filePath] is the selected markdown file, for the log line. */
data class TurnStateStatus(val text: String, val tooltip: String, val filePath: String?)

/** The banner strip's label and tooltip. */
data class TurnStateBanner(val label: String, val tooltip: String?)
