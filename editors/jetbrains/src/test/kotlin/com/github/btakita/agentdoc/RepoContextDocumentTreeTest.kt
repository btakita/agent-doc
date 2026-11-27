package com.github.btakita.agentdoc

import com.intellij.openapi.editor.Document
import com.intellij.ui.treeStructure.Tree
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Test
import java.awt.Component
import java.awt.Container
import java.lang.reflect.Proxy
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
        view.components.asSequence().flatMap(::children).filterIsInstance<Tree>().single().setSelectionRow(1)
        assertEquals(before, view.selection)

        view.show(RepoContextDocumentOutline("after", listOf(root, after)))
        assertEquals(after, view.selection)

        view.show(RepoContextDocumentOutline("ambiguous", listOf(root, after.copy(selectionKey = null))))
        assertNull(view.selection)
    }

    @Test
    fun `working task reveals its parent and rerenders status without a document edit`() = SwingUtilities.invokeAndWait {
        val view = RepoContextDocumentTree({}, {})
        val root = DocumentPromptTarget("document", "document", "Document", 1, 0, DocumentPromptAction.EXCHANGE)
        val parent = DocumentPromptTarget("target", "tracked:beta", "Review", 2, 1, DocumentPromptAction.EXCHANGE)
        fun task(state: String) =
            DocumentPromptTarget("task", "task", "Check the change", 4, 2, DocumentPromptAction.NAVIGATE, DocumentPromptTask("receipt", state))
        view.show(RepoContextDocumentOutline("same-revision", listOf(root, parent, task("submitted"))))
        val tree = view.components.asSequence().flatMap(::children).filterIsInstance<Tree>().single()
        tree.setSelectionRow(1)

        view.show(RepoContextDocumentOutline("same-revision", listOf(root, parent, task("running"))))
        assertEquals(parent, view.selection)
        assertEquals("Working · Check the change", tree.getPathForRow(2).lastPathComponent.toString())

        view.show(RepoContextDocumentOutline("same-revision", listOf(root, parent, task("answered"))))
        assertEquals(parent, view.selection)
        assertEquals("Answered · Check the change", tree.getPathForRow(2).lastPathComponent.toString())
    }

    private fun children(component: Component): Sequence<Component> = sequence {
        yield(component)
        if (component is Container) for (child in component.components) yieldAll(children(child))
    }
}
