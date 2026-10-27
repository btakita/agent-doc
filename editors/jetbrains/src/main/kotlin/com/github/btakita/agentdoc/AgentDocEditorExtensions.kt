package com.github.btakita.agentdoc

import com.github.btakita.agentdoc.shell.ComponentFold
import com.github.btakita.agentdoc.shell.ImplEditorExtensions
import com.github.btakita.agentdoc.shell.SlashCompletions
import com.github.btakita.agentdoc.shell.StructureNode
import com.github.btakita.agentdoc.shell.TurnStateBanner
import com.github.btakita.agentdoc.shell.TurnStateStatus
import com.intellij.codeInsight.completion.CompletionParameters
import com.intellij.openapi.Disposable
import com.intellij.openapi.editor.Document
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile

/**
 * GH #339 R1: the implementation half of every XML editor and language extension. The shell's
 * forwarders (`shell/ShellEditorForwarders.kt`) call these and build the platform objects.
 */
internal object AgentDocEditorExtensions : ImplEditorExtensions {
    override fun slashCompletions(parameters: CompletionParameters): SlashCompletions? =
        SlashCommandCompletions.complete(parameters)

    override fun componentFolds(root: PsiElement, document: Document): List<ComponentFold> =
        AgentDocComponentFoldingBuilder.foldRegions(root, document)

    override fun componentMarker(element: PsiElement): String? =
        AgentDocComponentLineMarkerProvider.markerName(element)

    override fun componentTooltip(element: PsiElement, name: String): String =
        AgentDocComponentLineMarkerProvider.tooltipFor(element, name)

    override fun toggleComponentFold(editor: Editor, element: PsiElement) =
        AgentDocComponentLineMarkerProvider.toggle(editor, element)

    override fun componentStructure(file: PsiFile): List<StructureNode> =
        AgentDocComponentStructureViewExtension.nodes(file)

    override fun turnStateStatus(project: Project): TurnStateStatus = TurnStateStatusBarWidget.status(project)

    override fun watchTurnState(project: Project, parent: Disposable, onChange: Runnable) =
        TurnStateStatusBarWidget.watch(project, parent, onChange)

    override fun requestTurnStateRefresh(project: Project, reason: String) =
        TurnStateStatusBarWidget.requestRefresh(project, reason)

    override fun turnStateBanner(project: Project, file: VirtualFile): TurnStateBanner? =
        TurnStateBannerProvider.banner(project, file)
}
