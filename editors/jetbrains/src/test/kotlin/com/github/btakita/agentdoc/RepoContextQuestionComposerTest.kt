package com.github.btakita.agentdoc

import com.intellij.openapi.util.SystemInfo
import org.junit.Assert.assertEquals
import org.junit.Assert.assertSame
import org.junit.Test
import java.awt.event.ActionEvent
import java.awt.event.InputEvent
import java.awt.event.KeyEvent
import javax.swing.KeyStroke
import javax.swing.SwingUtilities
import javax.swing.text.DefaultEditorKit

class RepoContextQuestionComposerTest {
    @Test
    fun `question accepts multiline typing while submission is disabled`() = SwingUtilities.invokeAndWait {
        var submissions = 0
        val composer = RepoContextQuestionComposer { submissions++ }
        val editor = composer.editor
        assertSame(editor, composer.label.labelFor)

        fun type(text: String) {
            editor.actionMap.get(DefaultEditorKit.defaultKeyTypedAction)
                .actionPerformed(ActionEvent(editor, ActionEvent.ACTION_PERFORMED, text))
        }

        fun press(stroke: KeyStroke) {
            editor.actionMap.get(editor.inputMap.get(stroke))
                .actionPerformed(ActionEvent(editor, ActionEvent.ACTION_PERFORMED, null))
        }

        val modifier = if (SystemInfo.isMac) InputEvent.META_DOWN_MASK else InputEvent.CTRL_DOWN_MASK
        val submit = KeyStroke.getKeyStroke(KeyEvent.VK_ENTER, modifier)
        type("First line")
        press(KeyStroke.getKeyStroke(KeyEvent.VK_ENTER, 0))
        type("Second line")
        press(submit)
        assertEquals("First line\nSecond line", editor.text)
        assertEquals(0, submissions)

        composer.submit.isEnabled = true
        press(submit)
        assertEquals(1, submissions)
        composer.submit.isEnabled = false
        type(" edited")
        assertEquals("First line\nSecond line edited", editor.text)
    }

    @Test
    fun `submission clears only the unchanged draft`() = SwingUtilities.invokeAndWait {
        val composer = RepoContextQuestionComposer {}
        val capturedDraft = "  Original question  "
        composer.editor.text = "$capturedDraft\nNew details"
        composer.clearSubmittedDraft(capturedDraft)
        assertEquals("$capturedDraft\nNew details", composer.editor.text)

        composer.editor.text = capturedDraft
        composer.clearSubmittedDraft(capturedDraft)
        assertEquals("", composer.editor.text)
    }
}
