package com.github.btakita.agentdoc

import com.intellij.openapi.editor.Document
import com.intellij.ui.treeStructure.Tree
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import java.awt.Component
import java.awt.Container
import java.lang.reflect.Proxy
import javax.swing.JButton
import javax.swing.SwingUtilities

class RepoContextDocumentTreeTest {
    @Test
    fun `prompt splice preserves unsaved editor text and refuses concurrent changes`() {
        val previousEdit = "Draft still in the editor. 🧭\n"
        val buffer = StringBuilder("<!-- agent:exchange -->\n${previousEdit}<!-- /agent:exchange -->\n")
        var stamp = 1L
        val document = Proxy.newProxyInstance(
            Document::class.java.classLoader,
            arrayOf(Document::class.java),
        ) { _, method, arguments ->
            when (method.name) {
                "getModificationStamp" -> stamp
                "getTextLength" -> buffer.length
                "insertString" -> {
                    buffer.insert(arguments[0] as Int, arguments[1] as CharSequence)
                    stamp++
                    null
                }
                else -> error("Unexpected document operation: ${method.name}")
            }
        } as Document
        val target = buffer.indexOf("<!-- /agent:exchange -->")
        val originalStamp = stamp
        val originalEdit = RepoContextPromptEdit(target, "❯ First question\n", DocumentPromptAction.EXCHANGE)
        originalEdit.applyTo(document, originalStamp)
        assertEquals(
            "<!-- agent:exchange -->\n${previousEdit}❯ First question\n<!-- /agent:exchange -->\n",
            buffer.toString(),
        )
        assertThrows(IllegalStateException::class.java) { originalEdit.applyTo(document, originalStamp) }
    }

    @Test
    fun `a new response keeps the selected tracked item available for another question`() = SwingUtilities.invokeAndWait {
        val view = RepoContextDocumentTree({}, {})
        val root = DocumentPromptTarget("document", "document", "Document", 1, 0, DocumentPromptAction.EXCHANGE)
        val before = DocumentPromptTarget("18:old", "tracked:beta", "[#beta] Review next action", 2, 1, DocumentPromptAction.EXCHANGE)
        val after = before.copy(id = "77:new", line = 6)
        view.show(RepoContextDocumentOutline("before", listOf(root, before)))
        view.selectTarget(before.id)
        assertEquals(before, view.selection)

        view.show(RepoContextDocumentOutline("after", listOf(root, after)))
        assertEquals(after, view.selection)

        view.show(RepoContextDocumentOutline("ambiguous", listOf(root, after.copy(selectionKey = null))))
        assertNull(view.selection)
    }

    @Test
    fun `inline composer follows the selected row and keeps drafts through refreshes`() = SwingUtilities.invokeAndWait {
        val view = RepoContextDocumentTree({}, {})
        val alpha = DocumentPromptTarget("old-alpha", "tracked:alpha", "Repeated title", 1, 0, DocumentPromptAction.EXCHANGE)
        val beta = DocumentPromptTarget("old-beta", "tracked:beta", "Repeated title", 2, 0, DocumentPromptAction.SUBPROMPT)
        val navigation = DocumentPromptTarget("read", "tracked:read", "A status row", 3, 0, DocumentPromptAction.NAVIGATE)
        view.show(RepoContextDocumentOutline("before", listOf(alpha, beta, navigation)), "session-a")
        val tree = tree(view).apply { setSize(360, 500) }
        val composer = view.question
        view.selectTarget(beta.id)
        tree.doLayout()
        val betaBounds = tree.getPathBounds(tree.selectionPath)
        assertTrue(composer.isVisible)
        assertTrue(composer.y >= betaBounds.y + betaBounds.height)
        assertTrue(composer.x > betaBounds.x)
        composer.editor.text = "Beta keeps its draft."
        composer.editor.caretPosition = 5

        val refreshedBeta = beta.copy(id = "new-beta", line = 6)
        view.show(RepoContextDocumentOutline("after", listOf(alpha, refreshedBeta, navigation)), "session-a")
        tree.doLayout()
        assertEquals(refreshedBeta, view.selection)
        assertEquals("Beta keeps its draft.", composer.editor.text)
        assertEquals(5, composer.editor.caretPosition)

        val composerPath = tree.getPathForRow(tree.getRowForPath(tree.selectionPath) + 1)
        tree.selectionPath = composerPath
        assertEquals(navigation, view.selection)
        view.selectTarget(alpha.id)
        assertEquals("", composer.editor.text)
        composer.editor.text = "Alpha keeps its own draft."
        view.selectTarget(navigation.id)
        tree.doLayout()
        assertFalse(composer.isVisible)
        view.selectTarget(refreshedBeta.id)
        assertEquals("Beta keeps its draft.", composer.editor.text)
        view.selectTarget(alpha.id)
        assertEquals("Alpha keeps its own draft.", composer.editor.text)

        view.show(RepoContextDocumentOutline("after", listOf(alpha, refreshedBeta, navigation)), "session-b")
        assertEquals("", composer.editor.text)
        view.show(RepoContextDocumentOutline("after", listOf(alpha, refreshedBeta, navigation)), "session-a")
        assertEquals("Alpha keeps its own draft.", composer.editor.text)
    }

    @Test
    fun `document tree lowers the native Lazily view controls`() = SwingUtilities.invokeAndWait {
        val native = EditorDerivation { derivation, _ ->
            assertEquals("document_prompts", derivation)
            """
            {
                "revision": "revision",
                "nodes": [
                    {"id":"root","label":"Root","line":1,"depth":0,"action":"queue"},
                    {"id":"child","label":"Child","line":2,"depth":1,"action":"navigate",
                     "task":{"request_id":"receipt","state":"waiting","label":"Waiting for you"}}
                ],
                "view": {
                    "document_tab":"Document", "repository_tab":"Repository",
                    "follow_editor_caret":"Follow current line", "linked_documents":"Links",
                    "prompt_label":"New prompt", "prompt_actions":{
                        "queue":{"label":"Add prompt","hint":"Add here","disabled":false},
                        "navigate":{"label":"Navigation only","hint":"Open only","disabled":true}
                    }
                }
            }
            """.trimIndent()
        }
        val outline = RepoContextDocumentPrompts(native).outline("# Root")
        val explorer = RepoContextDocumentTree({}, {})
        explorer.show(outline)
        assertEquals("Follow current line", explorer.followEditorCaret.text)
        assertEquals("New prompt", explorer.question.label.text)
        assertEquals(DocumentPromptControl("Add prompt", "Add here", false), explorer.promptControl)
        assertEquals("Waiting for you", outline.targets.last().task?.label)
        explorer.selectTarget("child")
        assertTrue(explorer.promptControl!!.disabled)
        assertFalse(explorer.question.isVisible)
    }

    @Test
    fun `turning off caret following keeps manual tree navigation available`() = SwingUtilities.invokeAndWait {
        var opened: DocumentPromptTarget? = null
        val view = RepoContextDocumentTree({}, { opened = it })
        val root = DocumentPromptTarget("document", "document", "Document", 1, 0, DocumentPromptAction.EXCHANGE)
        val target = DocumentPromptTarget("target", "tracked:target", "Review", 4, 1, DocumentPromptAction.EXCHANGE)
        view.show(RepoContextDocumentOutline("revision", listOf(root, target)))

        assertTrue(view.followEditorCaret.isSelected)
        view.followEditorCaret.doClick()
        assertFalse(view.followEditorCaret.isSelected)
        assertEquals(root, view.selection)

        val tree = tree(view)
        tree.selectionPath = (0 until tree.rowCount).map(tree::getPathForRow)
            .single { it.lastPathComponent.toString() == target.label }
        assertEquals(target, view.selection)
        children(view).filterIsInstance<JButton>().single { it.text == "Open in editor" }.doClick()
        assertEquals(target, opened)
    }

    private fun tree(view: RepoContextDocumentTree): Tree =
        view.components.asSequence().flatMap(::children).filterIsInstance<Tree>().single()

    @Test
    fun `working task reveals its parent and rerenders status without a document edit`() = SwingUtilities.invokeAndWait {
        val view = RepoContextDocumentTree({}, {})
        val root = DocumentPromptTarget("document", "document", "Document", 1, 0, DocumentPromptAction.EXCHANGE)
        val parent = DocumentPromptTarget("target", "tracked:beta", "Review", 2, 1, DocumentPromptAction.EXCHANGE)
        fun task(state: String) =
            DocumentPromptTarget("task", "task", "Check the change", 4, 2, DocumentPromptAction.NAVIGATE, DocumentPromptTask("receipt", state))
        view.show(RepoContextDocumentOutline("same-revision", listOf(root, parent, task("submitted"))))
        val tree = tree(view)
        fun taskLabel(): String = (0 until tree.rowCount).asSequence()
            .map { tree.getPathForRow(it).lastPathComponent.toString() }
            .single { it.endsWith(" · Check the change") }
        view.selectTarget(parent.id)

        view.show(RepoContextDocumentOutline("same-revision", listOf(root, parent, task("running"))))
        assertEquals(parent, view.selection)
        assertEquals("Working · Check the change", taskLabel())

        view.show(RepoContextDocumentOutline("same-revision", listOf(root, parent, task("answered"))))
        assertEquals(parent, view.selection)
        assertEquals("Answered · Check the change", taskLabel())
    }

    private fun children(component: Component): Sequence<Component> = sequence {
        yield(component)
        if (component is Container) for (child in component.components) yieldAll(children(child))
    }
}
