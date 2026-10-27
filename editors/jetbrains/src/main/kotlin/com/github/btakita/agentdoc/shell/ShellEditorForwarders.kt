package com.github.btakita.agentdoc.shell

import com.intellij.codeInsight.completion.CompletionContributor
import com.intellij.codeInsight.completion.CompletionParameters
import com.intellij.codeInsight.completion.CompletionProvider
import com.intellij.codeInsight.completion.CompletionResultSet
import com.intellij.codeInsight.completion.CompletionType
import com.intellij.codeInsight.completion.PlainPrefixMatcher
import com.intellij.codeInsight.daemon.GutterIconNavigationHandler
import com.intellij.codeInsight.daemon.LineMarkerInfo
import com.intellij.codeInsight.daemon.LineMarkerProvider
import com.intellij.codeInsight.lookup.LookupElementBuilder
import com.intellij.icons.AllIcons
import com.intellij.ide.DataManager
import com.intellij.ide.structureView.StructureViewExtension
import com.intellij.ide.structureView.StructureViewTreeElement
import com.intellij.lang.ASTNode
import com.intellij.lang.folding.FoldingBuilderEx
import com.intellij.lang.folding.FoldingDescriptor
import com.intellij.navigation.ItemPresentation
import com.intellij.openapi.Disposable
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.editor.Document
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.editor.markup.GutterIconRenderer
import com.intellij.openapi.fileEditor.FileEditor
import com.intellij.openapi.fileEditor.OpenFileDescriptor
import com.intellij.openapi.project.DumbAware
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.Disposer
import com.intellij.openapi.util.TextRange
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.openapi.wm.StatusBar
import com.intellij.openapi.wm.StatusBarWidget
import com.intellij.openapi.wm.StatusBarWidgetFactory
import com.intellij.patterns.PlatformPatterns
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.ui.EditorNotificationProvider
import com.intellij.util.ProcessingContext
import java.awt.event.MouseEvent
import java.util.function.Function
import javax.swing.Icon
import javax.swing.JComponent

/*
 * GH #339 R1: the shell forwarders for every XML editor and language extension
 * (`plugin.xml` and `agent-doc-markdown.xml`). Each one asks the active implementation
 * generation for data through [ImplEditorExtensions] and builds the platform object itself.
 * With no active generation (the plugin is unloading) each answers "nothing", as the
 * implementation did once its generation retired.
 */

private fun extensions(): ImplEditorExtensions? = ShellGenerationHost.getInstance().editorExtensions()

/** `completion.contributor`: slash-command completion in markdown files. */
class ShellSlashCommandCompletionContributor : CompletionContributor(), DumbAware {
    init {
        extend(
            CompletionType.BASIC,
            PlatformPatterns.psiElement(),
            object : CompletionProvider<CompletionParameters>() {
                override fun addCompletions(
                    parameters: CompletionParameters,
                    context: ProcessingContext,
                    result: CompletionResultSet,
                ) {
                    val completions = extensions()?.slashCompletions(parameters) ?: return
                    // The full slash-prefixed text is the prefix matcher, so IntelliJ filters
                    // the results correctly.
                    val slashResult = result.withPrefixMatcher(PlainPrefixMatcher(completions.prefix))
                    for (item in completions.items) {
                        slashResult.addElement(
                            LookupElementBuilder.create(item.name)
                                .withTailText(item.tailText, true)
                                .withTypeText(item.typeText, true)
                                .withBoldness(item.bold),
                        )
                    }
                }
            },
        )
    }
}

/** `lang.foldingBuilder` (Markdown): session-component fold regions. */
class ShellComponentFoldingBuilder : FoldingBuilderEx(), DumbAware {
    override fun buildFoldRegions(root: PsiElement, document: Document, quick: Boolean): Array<FoldingDescriptor> {
        val folds = extensions()?.componentFolds(root, document).orEmpty()
        if (folds.isEmpty()) return FoldingDescriptor.EMPTY_ARRAY
        val node = root.containingFile?.node ?: return FoldingDescriptor.EMPTY_ARRAY
        return folds
            .map { fold -> FoldingDescriptor(node, TextRange(fold.start, fold.end), null, fold.placeholder) }
            .toTypedArray()
    }

    override fun getPlaceholderText(node: ASTNode): String = "<!-- agent:… -->"

    override fun isCollapsedByDefault(node: ASTNode): Boolean = false
}

/** `codeInsight.lineMarkerProvider` (Markdown): a gutter icon on each component open marker. */
class ShellComponentLineMarkerProvider : LineMarkerProvider, DumbAware {
    override fun getLineMarkerInfo(element: PsiElement): LineMarkerInfo<*>? {
        val name = extensions()?.componentMarker(element) ?: return null
        val range = element.textRange ?: return null
        return LineMarkerInfo(
            element,
            range,
            AllIcons.Nodes.Template,
            { e -> extensions()?.componentTooltip(e, name) ?: "agent:$name" },
            ToggleComponentFold,
            GutterIconRenderer.Alignment.LEFT,
            { "agent:$name component" },
        )
    }

    private object ToggleComponentFold : GutterIconNavigationHandler<PsiElement> {
        override fun navigate(e: MouseEvent, elt: PsiElement) {
            val editor = DataManager.getInstance().getDataContext(e.component)
                .getData(CommonDataKeys.EDITOR) ?: return
            extensions()?.toggleComponentFold(editor, elt)
        }
    }
}

/**
 * `lang.structureViewExtension`: a session document's components in the Markdown structure
 * view, built as shell nodes from the implementation's [StructureNode] data.
 */
class ShellComponentStructureViewExtension : StructureViewExtension {
    override fun getType(): Class<out PsiElement> = PsiFile::class.java

    override fun getChildren(parent: PsiElement): Array<StructureViewTreeElement> {
        val file = parent as? PsiFile ?: return EMPTY
        val vFile = file.virtualFile ?: return EMPTY
        val nodes = extensions()?.componentStructure(file).orEmpty()
        if (nodes.isEmpty()) return EMPTY
        return nodes.map { ShellStructureElement(file.project, vFile, it) }.toTypedArray()
    }

    override fun getCurrentEditorElement(editor: Editor, parent: PsiElement): Any? = null

    private class ShellStructureElement(
        val project: Project,
        val file: VirtualFile,
        val node: StructureNode,
    ) : StructureViewTreeElement, ItemPresentation {
        override fun getValue(): Any = node.key
        override fun getPresentation(): ItemPresentation = this
        override fun getPresentableText(): String = node.presentableText
        override fun getLocationString(): String? = node.locationString
        override fun getIcon(unused: Boolean): Icon = when (node.kind) {
            StructureNodeKind.COMPONENT -> AllIcons.Nodes.Template
            StructureNodeKind.HEADING -> AllIcons.Nodes.Folder
            StructureNodeKind.LIST_ITEM -> AllIcons.Nodes.Property
        }

        override fun getChildren(): Array<StructureViewTreeElement> =
            node.children.map { ShellStructureElement(project, file, it) }.toTypedArray()

        override fun navigate(requestFocus: Boolean) {
            OpenFileDescriptor(project, file, node.offset).navigate(requestFocus)
        }

        override fun canNavigate(): Boolean = file.isValid
        override fun canNavigateToSource(): Boolean = file.isValid

        override fun equals(other: Any?): Boolean = other is ShellStructureElement && other.node.key == node.key
        override fun hashCode(): Int = node.key.hashCode()
    }

    private companion object {
        val EMPTY: Array<StructureViewTreeElement> = emptyArray()
    }
}

/**
 * `statusBarWidgetFactory`: the turn-state widget that makes Project Controller turn-state
 * coordination visible in the editor (goal 1). The widget is a shell object; it reads the
 * implementation's event-driven turn-state cache, so painting never calls native projection APIs
 * on a Swing timer. Parity with the VS Code status-bar indicator
 * (`specs/14-realtime-workflow.md` § Editor Parity Requirement).
 */
class ShellTurnStateStatusBarWidgetFactory : StatusBarWidgetFactory {
    override fun getId(): String = ShellTurnStateStatusBarWidget.WIDGET_ID
    override fun getDisplayName(): String = "Agent Doc Turn State"
    override fun isAvailable(project: Project): Boolean = true
    override fun createWidget(project: Project): StatusBarWidget = ShellTurnStateStatusBarWidget(project)
    override fun disposeWidget(widget: StatusBarWidget) = widget.dispose()
    override fun canBeEnabledOn(statusBar: StatusBar): Boolean = true
}

class ShellTurnStateStatusBarWidget(private val project: Project) :
    StatusBarWidget, StatusBarWidget.TextPresentation {

    companion object {
        const val WIDGET_ID = "com.github.btakita.agentdoc.TurnStateStatusBar"
        internal const val BRAND = "agent-doc"
        internal const val DEFAULT_TOOLTIP =
            "Agent Doc turn state — the Project Controller's authoritative turn phase for this document"
        private val LOG = Logger.getInstance(ShellTurnStateStatusBarWidget::class.java)
    }

    private var statusBar: StatusBar? = null
    private var watch: Disposable? = null
    // Never empty: an empty TextPresentation makes the platform build a zero-width
    // TextPanel at creation that a later updateWidget won't re-grow, so the widget
    // stays invisible. Seed with the brand so the component has a paintable size
    // from the first render.
    private var widgetText: String = BRAND
    private var widgetTooltip: String = DEFAULT_TOOLTIP

    override fun ID(): String = WIDGET_ID

    override fun getPresentation(): StatusBarWidget.WidgetPresentation = this

    override fun install(statusBar: StatusBar) {
        this.statusBar = statusBar
        val extensions = extensions() ?: return
        val parent = Disposer.newDisposable("agent-doc turn-state widget")
        watch = parent
        extensions.watchTurnState(project, parent) { refreshFromCache() }
        LOG.info("[turn-widget] installed on status bar; using event-driven turn-state cache")
        refreshFromCache()
        extensions.requestTurnStateRefresh(project, "statusbar-install")
    }

    override fun dispose() {
        watch?.let(Disposer::dispose)
        watch = null
        statusBar = null
    }

    private fun refreshFromCache() {
        val status = extensions()?.turnStateStatus(project) ?: return
        if (status.text != widgetText || status.tooltip != widgetTooltip) {
            LOG.info("[turn-widget] refresh: file=${status.filePath ?: "(none)"} text=\"${status.text}\"")
            widgetText = status.text
            widgetTooltip = status.tooltip
            statusBar?.updateWidget(WIDGET_ID)
        }
    }

    override fun getText(): String = widgetText
    override fun getAlignment(): Float = 0f
    override fun getTooltipText(): String = widgetTooltip
}

/**
 * `editorNotificationProvider`: the editor banner that surfaces the Project Controller's
 * authoritative turn phase across the top of an agent-doc markdown file, built by
 * [ShellTurnStateStrips] from the implementation's cached [TurnStateBanner].
 */
class ShellTurnStateBannerProvider : EditorNotificationProvider, DumbAware {
    override fun collectNotificationData(
        project: Project,
        file: VirtualFile,
    ): Function<in FileEditor, out JComponent?>? {
        val banner = extensions()?.turnStateBanner(project, file) ?: return null
        // The platform may apply this factory on the EDT after the generation retired
        // (`#duplicateturnbanner`), so the strip builder re-checks instead of trusting this one.
        return Function { _ -> ShellTurnStateStrips.createStrip(banner.label, banner.tooltip) }
    }
}
