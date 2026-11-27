package com.github.btakita.agentdoc

import com.google.gson.JsonObject
import com.intellij.openapi.editor.Document
import com.intellij.ui.components.JBLabel
import com.intellij.ui.components.JBScrollPane
import com.intellij.ui.treeStructure.Tree
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

internal data class RepoContextDocumentOutline(val revision: String, val targets: List<DocumentPromptTarget>)

internal data class RepoContextPromptEdit(val offset: Int, val text: String, val action: DocumentPromptAction) {
    fun applyTo(document: Document, stamp: Long) {
        check(document.modificationStamp == stamp && offset in 0..document.textLength) {
            "The session changed. Select the target and try again."
        }
        document.insertString(offset, text)
    }
}

internal class RepoContextDocumentPrompts(private val native: EditorDerivation = EditorDerivation.Native) {
    fun outline(source: String, filePath: String? = null): RepoContextDocumentOutline {
        val output = derive(source) { filePath?.let { addProperty("file", it) } }
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
                    node.getAsJsonObject("task")?.let { task ->
                        DocumentPromptTask(task.get("request_id").asString, task.get("state").asString)
                    },
                )
            },
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
    onSelection: () -> Unit,
    onOpen: (DocumentPromptTarget) -> Unit,
) : RepoContextTypography(BorderLayout(0, JBUI.scale(4))) {
    private val tree = Tree().apply {
        isRootVisible = false
        accessibleContext.accessibleName = "Session document"
        selectionModel.selectionMode = TreeSelectionModel.SINGLE_TREE_SELECTION
    }
    private val hint = JBLabel("Choose an Agent Doc session.")
    private val open = JButton("Open in editor").apply {
        isEnabled = false
        addActionListener { selection?.let(onOpen) }
    }
    private var revision: RepoContextDocumentOutline? = null

    val selection: DocumentPromptTarget?
        get() = (tree.lastSelectedPathComponent as? DefaultMutableTreeNode)?.userObject as? DocumentPromptTarget

    init {
        add(JBScrollPane(tree), BorderLayout.CENTER)
        add(JPanel(BorderLayout(JBUI.scale(4), 0)).apply {
            add(hint, BorderLayout.CENTER)
            add(open, BorderLayout.EAST)
        }, BorderLayout.SOUTH)
        useCompactFont(tree, hint, open)
        tree.model = DefaultTreeModel(DefaultMutableTreeNode())
        tree.addTreeSelectionListener {
            val selected = selection
            open.isEnabled = selected != null
            hint.text = selected?.action?.hint ?: "Select a document section or item."
            onSelection()
        }
    }

    fun clear(message: String) {
        revision = null
        tree.model = DefaultTreeModel(DefaultMutableTreeNode())
        hint.text = message
        open.isEnabled = false
    }

    fun show(outline: RepoContextDocumentOutline) {
        if (outline == revision) return
        val previouslyWorking = revision?.targets.orEmpty()
            .mapNotNull { it.task?.takeIf { task -> task.state == "running" }?.requestId }
            .toSet()
        val previous = (tree.model.root as DefaultMutableTreeNode)
        val expanded = tree.getExpandedDescendants(TreePath(previous.path))?.toList()
            ?.mapNotNull { path ->
                ((path.lastPathComponent as? DefaultMutableTreeNode)?.userObject as? DocumentPromptTarget)?.selectionKey
            }.orEmpty()
        val selected = selection?.selectionKey
        val initial = revision == null
        val root = DefaultMutableTreeNode()
        val lineage = mutableListOf<Pair<Int, DefaultMutableTreeNode>>()
        val nodes = mutableMapOf<String, DefaultMutableTreeNode>()
        val newWorking = mutableListOf<DefaultMutableTreeNode>()
        for (target in outline.targets) {
            while (lineage.isNotEmpty() && lineage.last().first >= target.depth) lineage.removeLast()
            val branch = DefaultMutableTreeNode(target)
            (lineage.lastOrNull()?.second ?: root).add(branch)
            lineage.add(target.depth to branch)
            target.selectionKey?.let { nodes[it] = branch }
            if (target.task?.let { it.state == "running" && it.requestId !in previouslyWorking } == true) {
                newWorking.add(branch)
            }
        }
        revision = outline
        tree.model = DefaultTreeModel(root)
        tree.expandPath(TreePath(root.path))
        for (id in expanded) nodes[id]?.let { tree.expandPath(TreePath(it.path)) }
        val first = if (root.childCount > 0) root.getChildAt(0) as DefaultMutableTreeNode else null
        if (first != null) tree.expandPath(TreePath(first.path))
        for (branch in newWorking) tree.expandPath(TreePath(branch.path).parentPath)
        val current = nodes[selected] ?: first.takeIf { initial }
        current?.let { tree.selectionPath = TreePath(it.path) }
        if (outline.targets.isEmpty()) hint.text = "No document sections are available."
    }
}
