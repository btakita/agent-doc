package com.github.btakita.agentdoc

import com.google.gson.JsonObject
import com.intellij.openapi.editor.Document
import com.intellij.openapi.ui.ComboBox
import com.intellij.ui.components.JBLabel
import com.intellij.ui.components.JBScrollPane
import com.intellij.util.ui.JBUI
import java.awt.BorderLayout
import javax.swing.JButton
import javax.swing.JPanel
import javax.swing.tree.DefaultMutableTreeNode
import javax.swing.tree.DefaultTreeModel
import javax.swing.tree.TreePath
import javax.swing.tree.TreeSelectionModel

internal enum class DocumentPromptAction(val button: String, val hint: String) {
    EXCHANGE("Ask agent", "Send about this target in Exchange."),
    QUEUE("Queue prompt", "Add to this run under its queue settings."),
    SUBPROMPT("Add subprompt", "Add a child prompt under this row."),
    NAVIGATE("Ask agent", "Navigation only. Select another row to ask."),
}

internal data class DocumentPromptTarget(
    val id: String,
    val selectionKey: String?,
    val label: String,
    val line: Int,
    val depth: Int,
    val action: DocumentPromptAction,
    val task: DocumentPromptTask? = null,
    val endLine: Int? = null,
) {
    override fun toString(): String = task?.let { "${it.label} · $label" } ?: label
}

internal data class DocumentPromptTask(val requestId: String, val state: String) {
    val label: String
        get() = when (state) {
            "running" -> "Working"
            "waiting" -> "Waiting for you"
            "interrupted" -> "Interrupted"
            else -> state.replaceFirstChar { it.uppercase() }
        }
}

internal data class RepoContextDocumentOutline(
    val revision: String,
    val targets: List<DocumentPromptTarget>,
    val links: List<String> = emptyList(),
)

internal data class RepoContextPromptEdit(val offset: Int, val text: String, val action: DocumentPromptAction) {
    fun applyTo(document: Document, stamp: Long) {
        check(document.modificationStamp == stamp && offset in 0..document.textLength) {
            "The session changed. Select the target and try again."
        }
        document.insertString(offset, text)
    }
}

internal class RepoContextDocumentPrompts(private val native: EditorDerivation = EditorDerivation.Native) {
    fun outline(source: String, filePath: String? = null, projectRoot: String? = null): RepoContextDocumentOutline {
        val output = derive(source) {
            filePath?.let { addProperty("file", it) }
            projectRoot?.let { addProperty("project_root", it) }
        }
        return RepoContextDocumentOutline(
            output.get("revision").asString,
            output.getAsJsonArray("nodes").map { entry ->
                val node = entry.asJsonObject
                DocumentPromptTarget(
                    node.get("id").asString,
                    node.get("selection_key")?.takeUnless { it.isJsonNull }?.asString,
                    node.get("label").asString,
                    node.get("line").asInt,
                    node.get("depth").asInt,
                    action(node),
                    task = node.getAsJsonObject("task")?.let { task ->
                        DocumentPromptTask(task.get("request_id").asString, task.get("state").asString)
                    },
                    endLine = node.get("end_line")?.takeUnless { it.isJsonNull }?.asInt,
                )
            },
            links = output.getAsJsonArray("links")?.map { it.asString }.orEmpty(),
        )
    }

    fun edit(source: String, revision: String, target: String, draft: String, receipt: String): RepoContextPromptEdit {
        val output = derive(source) {
            add("submission", JsonObject().apply {
                addProperty("revision", revision)
                addProperty("target", target)
                addProperty("draft", draft)
                addProperty("request_id", receipt)
            })
        }
        return RepoContextPromptEdit(output.get("offset").asInt, output.get("text").asString, action(output))
    }

    fun workerAnswer(source: String, receipt: String): RepoContextAnswer? {
        val output = derive(source) { addProperty("answer", receipt) }
        return output.get("offset")?.asInt?.let { offset -> RepoContextAnswer(offset, output.get("text").asString) }
    }

    private fun derive(source: String, fields: JsonObject.() -> Unit = {}): JsonObject {
        val input = JsonObject().apply {
            addProperty("source", source)
            fields()
        }
        val output = native.derive("document_prompts", input.toString())
            ?: error("The loaded Agent Doc native library does not support the document prompt tree.")
        if (output == "null") return JsonObject()
        val json = RepoContextModel.decode(output)
        json.get("error")?.let { error(it.asString) }
        return json
    }

    private fun action(json: JsonObject): DocumentPromptAction = when (val name = json.get("action").asString) {
        "exchange" -> DocumentPromptAction.EXCHANGE
        "queue" -> DocumentPromptAction.QUEUE
        "subprompt" -> DocumentPromptAction.SUBPROMPT
        "navigate" -> DocumentPromptAction.NAVIGATE
        else -> error("Unknown document prompt action: $name")
    }
}

internal class RepoContextDocumentTree(
    private val onSelection: () -> Unit,
    onOpen: (DocumentPromptTarget) -> Unit,
    onSubmit: () -> Unit = {},
    onOpenLink: (String) -> Unit = {},
) : RepoContextTypography(BorderLayout(0, JBUI.scale(4))) {
    val question = RepoContextQuestionComposer(onSubmit)
    private val tree = RepoContextInlinePromptTree(question).apply {
        isRootVisible = false
        accessibleContext.accessibleName = "Session document"
        selectionModel.selectionMode = TreeSelectionModel.SINGLE_TREE_SELECTION
    }
    private val hint = JBLabel("Choose an Agent Doc session.")
    private val open = JButton("Open in editor").apply {
        isEnabled = false
        addActionListener { selection?.let(onOpen) }
    }
    private val linkedDocuments = ComboBox<String>().apply {
        preferredSize = JBUI.size(180, preferredSize.height)
        accessibleContext.accessibleName = "Linked documents"
    }
    private val openLink = JButton("Open link").apply {
        addActionListener { (linkedDocuments.selectedItem as? String)?.let(onOpenLink) }
    }
    private val linksLabel = JBLabel("Linked documents").apply { labelFor = linkedDocuments }
    private val links = JPanel(BorderLayout(JBUI.scale(4), 0)).apply {
        add(linksLabel, BorderLayout.WEST)
        add(linkedDocuments, BorderLayout.CENTER)
        add(openLink, BorderLayout.EAST)
        isVisible = false
    }
    private var revision: RepoContextDocumentOutline? = null
    private var scope = ""
    private var replacingModel = false
    private var draftKey: DraftKey? = null
    private val drafts = mutableMapOf<DraftKey, String>()
    private var targetBranches = emptyMap<String, DefaultMutableTreeNode>()

    val selection: DocumentPromptTarget?
        get() = selectedBranch()?.userObject as? DocumentPromptTarget
    val isEditingPrompt: Boolean
        get() = question.editor.isFocusOwner

    init {
        add(links, BorderLayout.NORTH)
        add(JBScrollPane(tree), BorderLayout.CENTER)
        add(JPanel(BorderLayout(JBUI.scale(4), 0)).apply {
            add(hint, BorderLayout.CENTER)
            add(open, BorderLayout.EAST)
        }, BorderLayout.SOUTH)
        useCompactFont(tree, hint, open, question.label, question.editor, question.submit, linksLabel, linkedDocuments, openLink)
        tree.model = DefaultTreeModel(DefaultMutableTreeNode())
        tree.addTreeSelectionListener {
            if (!replacingModel) updateSelection()
        }
    }

    private data class DraftKey(val session: String, val target: String, val revision: String?)

    private fun selectedBranch(): DefaultMutableTreeNode? =
        tree.lastSelectedPathComponent as? DefaultMutableTreeNode

    fun selectTarget(targetId: String) {
        val branch = targetBranches[targetId] ?: return
        val path = TreePath(branch.path)
        if (tree.selectionPath == path) return
        tree.selectionPath = path
        tree.scrollPathToVisible(path)
    }

    private fun updateSelection() {
        val branch = selectedBranch()
        val selected = branch?.userObject as? DocumentPromptTarget
        val promptTarget = selected?.takeIf { it.action != DocumentPromptAction.NAVIGATE }
        val nextKey = promptTarget?.let { target ->
            DraftKey(scope, target.selectionKey ?: target.id, if (target.selectionKey == null) revision?.revision else null)
        }
        if (nextKey != draftKey) {
            draftKey?.let { previous ->
                val text = question.editor.text
                if (text.isEmpty()) drafts.remove(previous) else drafts[previous] = text
            }
            draftKey = nextKey
            question.editor.text = nextKey?.let(drafts::get).orEmpty()
        }
        tree.putComposerUnder(branch.takeIf { promptTarget != null })
        open.isEnabled = selected != null
        hint.text = selected?.action?.hint ?: "Select a document section or item."
        onSelection()
    }

    fun clear(message: String) {
        showLinks(emptyList())
        revision = null
        targetBranches = emptyMap()
        replacingModel = true
        tree.model = DefaultTreeModel(DefaultMutableTreeNode())
        replacingModel = false
        updateSelection()
        hint.text = message
    }

    private fun showLinks(paths: List<String>) {
        val selected = linkedDocuments.selectedItem as? String
        linkedDocuments.removeAllItems()
        paths.forEach { linkedDocuments.addItem(it) }
        if (selected != null && selected in paths) linkedDocuments.selectedItem = selected
        links.isVisible = paths.isNotEmpty()
    }

    fun show(outline: RepoContextDocumentOutline, draftScope: String = "") {
        if (outline == revision && draftScope == scope) return
        val previouslyWorking = revision?.targets.orEmpty()
            .mapNotNull { it.task?.takeIf { task -> task.state == "running" }?.requestId }
            .toSet()
        val previous = (tree.model.root as DefaultMutableTreeNode)
        val expanded = tree.getExpandedDescendants(TreePath(previous.path))?.toList()
            ?.mapNotNull { path ->
                ((path.lastPathComponent as? DefaultMutableTreeNode)?.userObject as? DocumentPromptTarget)?.selectionKey
            }.orEmpty()
        val sameSession = draftScope == scope
        val selected = selection?.selectionKey.takeIf { sameSession }
        val initial = revision == null || !sameSession
        val focused = question.editor.isFocusOwner
        val previousDraft = draftKey
        val root = DefaultMutableTreeNode()
        val lineage = mutableListOf<Pair<Int, DefaultMutableTreeNode>>()
        val nodes = mutableMapOf<String, DefaultMutableTreeNode>()
        val branches = mutableMapOf<String, DefaultMutableTreeNode>()
        val newWorking = mutableListOf<DefaultMutableTreeNode>()
        for (target in outline.targets) {
            while (lineage.isNotEmpty() && lineage.last().first >= target.depth) lineage.removeLast()
            val branch = DefaultMutableTreeNode(target)
            (lineage.lastOrNull()?.second ?: root).add(branch)
            lineage.add(target.depth to branch)
            target.selectionKey?.let { nodes[it] = branch }
            branches[target.id] = branch
            if (target.task?.let { it.state == "running" && it.requestId !in previouslyWorking } == true) {
                newWorking.add(branch)
            }
        }
        if (outline.links != revision?.links || !sameSession) showLinks(outline.links)
        revision = outline
        scope = draftScope
        targetBranches = branches
        replacingModel = true
        tree.model = DefaultTreeModel(root)
        tree.expandPath(TreePath(root.path))
        for (id in expanded) nodes[id]?.let { tree.expandPath(TreePath(it.path)) }
        val first = if (root.childCount > 0) root.getChildAt(0) as DefaultMutableTreeNode else null
        if (first != null) tree.expandPath(TreePath(first.path))
        for (branch in newWorking) tree.expandPath(TreePath(branch.path).parentPath)
        val current = nodes[selected] ?: first.takeIf { initial }
        current?.let { tree.selectionPath = TreePath(it.path) }
        replacingModel = false
        updateSelection()
        if (focused && draftKey != null && draftKey == previousDraft && !question.editor.isFocusOwner) {
            question.editor.requestFocusInWindow()
        }
        if (outline.targets.isEmpty()) hint.text = "No document sections are available."
    }
}
