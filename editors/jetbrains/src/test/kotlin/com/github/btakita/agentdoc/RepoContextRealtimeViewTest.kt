package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeFalse
import org.junit.Assume.assumeNotNull
import org.junit.Test
import java.awt.GraphicsEnvironment
import java.awt.KeyboardFocusManager
import java.awt.event.FocusAdapter
import java.awt.event.FocusEvent
import java.awt.event.InputMethodEvent
import java.nio.file.Path
import java.text.AttributedString
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import javax.swing.DefaultListModel
import javax.swing.JFrame
import javax.swing.JList
import javax.swing.JPanel
import javax.swing.JTextField
import javax.swing.SwingUtilities

class RepoContextRealtimeViewTest {
    private val first = RepoContextLocation(Path.of("first.md"), 1, 2, "First", "heading")
    private val second = RepoContextLocation(Path.of("second.md"), 3, 5, "Second", "heading")
    private val insert = RepoContextLocation(Path.of("insert.md"), 4, 4, "Insert", "heading")

    @Test
    fun `Swing reconciliation preserves row identity and the composing question draft`() {
        SwingUtilities.invokeAndWait {
            val model = DefaultListModel<RepoContextLocation>()
            val list = JList(model)
            val field = JTextField("draft question")
            val draft = RepoContextQuestionDraft(field)
            field.select(2, 7)
            val originalDocument = field.document
            draft.inputMethodTextChanged(InputMethodEvent(
                field, InputMethodEvent.INPUT_METHOD_TEXT_CHANGED,
                AttributedString("かな").iterator, 0, null, null,
            ))
            setRepoContextRows("results", list, model, listOf(first, second))
            list.selectedIndex = 0
            setRepoContextRows("results", list, model, listOf(second.copy(), insert, first.copy()))

            assertSame(first, list.selectedValue)
            assertSame(second, model.get(0))
            assertSame(originalDocument, field.document)
            assertEquals("draft question", field.text)
            assertEquals(2, field.selectionStart)
            assertEquals(7, field.selectionEnd)
            assertNull(draft.submission())

            draft.inputMethodTextChanged(InputMethodEvent(
                field, InputMethodEvent.INPUT_METHOD_TEXT_CHANGED,
                AttributedString("かな").iterator, 2, null, null,
            ))
            assertEquals("draft question", draft.submission())
            field.text = "later draft"
            draft.clearIfUnchanged("draft question")
            assertEquals("later draft", field.text)
        }
    }

    @Test
    fun `visible Swing question keeps focus through keyed reconciliation`() {
        assumeFalse(GraphicsEnvironment.isHeadless())
        lateinit var window: JFrame
        lateinit var field: JTextField
        lateinit var list: JList<RepoContextLocation>
        val model = DefaultListModel<RepoContextLocation>()
        val focused = CountDownLatch(1)
        SwingUtilities.invokeAndWait {
            field = JTextField("saved draft", 20)
            field.addFocusListener(object : FocusAdapter() {
                override fun focusGained(event: FocusEvent) = focused.countDown()
            })
            list = JList(model)
            window = JFrame("Explorer focus test")
            window.contentPane = JPanel().apply { add(field); add(list) }
            window.pack()
            window.isVisible = true
            field.requestFocusInWindow()
            setRepoContextRows("results", list, model, listOf(first, second))
        }
        try {
            assertTrue("Expected the Explorer question to receive window focus", focused.await(5, TimeUnit.SECONDS))
            SwingUtilities.invokeAndWait {
                field.caretPosition = 4
                setRepoContextRows("results", list, model, listOf(second, insert, first))
                assertSame(field, KeyboardFocusManager.getCurrentKeyboardFocusManager().focusOwner)
                assertEquals(4, field.caretPosition)
                assertEquals("saved draft", field.text)
            }
        } finally {
            SwingUtilities.invokeAndWait { window.dispose() }
        }
    }

    @Test
    fun `shared View replay admits one authorized JetBrains submit`() {
        val runtime = RepoContextRealtimeView.open()
        assumeNotNull(runtime)
        val view = requireNotNull(runtime)
        view.publish(listOf(first, second), listOf(first), true, true, false)
        assertEquals(1, view.relatedCount())
        // An unavailable host action does not reserve the event ID.
        assertFalse(view.accepts("submit-question", "submit", "wrong", "draft", "submit-1"))
        var authorizedActions = 0
        if (view.accepts("submit-question", "submit", "ask", "draft", "submit-1")) authorizedActions++

        view.publish(listOf(second, insert, first), listOf(first, second), true, true, false)
        assertEquals(2, view.relatedCount())
        assertFalse(view.accepts("submit-question", "submit", "ask", "draft", "stale", displayedRevision = 1))
        view.publish(listOf(second, insert, first), listOf(first, second), true, true, false, reconnect = true)
        if (view.accepts("submit-question", "submit", "ask", "draft", "submit-1", displayedRevision = 1)) authorizedActions++
        assertEquals(1, authorizedActions)
        assertTrue(view.accepts(repoContextRowId("results", first), "activate", "select"))

        view.publish(listOf(second), emptyList(), false, true, false)
        assertEquals(0, view.relatedCount())
        assertFalse(view.accepts(repoContextRowId("results", first), "activate", "select"))
        assertFalse(view.accepts("submit-question", "submit", "ask", "draft"))
    }
}
