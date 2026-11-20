package com.github.btakita.agentdoc

import java.nio.file.Files
import java.nio.file.Paths
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

class EditorInputEvidenceTest {
    @Test
    fun `stray p before run marker remains an exact bounded insertion in a large attached document`() {
        val marker = "<!-- /agent:run -->"
        val before = "x".repeat(112_000) + "\n" + marker + "\n"
        val offset = before.indexOf(marker)
        val edit =
            CapturedLocalEditorEdit(
                offsetUtf16 = offset,
                oldFragment = "",
                newFragment = "p",
                projectionEpoch = 41L,
                beforeLengthUtf16 = before.length,
                agentLineage = "patch-41",
                inputEvidence = evidence(1L, offset + 1, offset + 1, offset + 1),
            )

        val prepared = prepareLocalEditorEditsUtil(ReplicaShadow(before), listOf(edit))
        assertNotNull(prepared)
        assertTrue(prepared!!.resultingShadow.toString().contains("p$marker"))
        val receipt = editorInputReceiptUtil(listOf(edit)) { value -> "h${value.length}" }!!
        assertTrue(receipt.contains("shape=insertion"))
        assertTrue(receipt.contains("range=$offset..$offset"))
        assertTrue(receipt.contains("old_chars=0,new_chars=1"))
        assertTrue(receipt.contains("epoch=41,lineage=patch-41"))
    }

    @Test
    fun `selection sized hotkey replacement is observed factually and never classified as corruption`() {
        val selected = "s".repeat(576)
        val marker = "<!-- /agent:run -->"
        val prefix = "x".repeat(111_000)
        val before = prefix + selected + marker
        val edit =
            CapturedLocalEditorEdit(
                offsetUtf16 = prefix.length,
                oldFragment = selected,
                newFragment = "p",
                projectionEpoch = 52L,
                beforeLengthUtf16 = before.length,
                inputEvidence = evidence(2L, prefix.length + 1, prefix.length + 1, prefix.length + 1),
            )

        val prepared = prepareLocalEditorEditsUtil(ReplicaShadow(before), listOf(edit))
        assertNotNull(prepared)
        assertEquals(prefix + "p" + marker, prepared!!.resultingShadow.toString())
        assertEquals(EditorInputEditShape.Replacement, editorInputEditShapeUtil(576, 1))
        val receipt = editorInputReceiptUtil(listOf(edit)) { value -> "h${value.length}" }!!
        assertTrue(receipt.contains("shape=replacement"))
        assertTrue(receipt.contains("old_chars=576,new_chars=1"))
        assertTrue(receipt.contains("focus=true"))
        assertTrue(receipt.contains("writable=true"))
        assertFalse(receipt.contains("corrupt", ignoreCase = true))
        assertFalse(receipt.contains("lost", ignoreCase = true))
    }

    @Test
    fun `input receipt is absent for legacy synthetic splices`() {
        assertEquals(null, editorInputReceiptUtil(listOf(CapturedLocalEditorEdit(0, "", "p", 0L))) { "hash" })
    }

    @Test
    fun `EDT evidence capture stays bounded and never reads the document text`() {
        val sourcePath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/EditorInputEvidence.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorInputEvidence.kt"),
        ).first { Files.exists(it) }
        val source = Files.readString(sourcePath)
        val capture = source.substringAfter("fun capture(project: Project, document: Document)")
            .substringBefore("private fun focusBelongsToEditor")

        assertTrue(capture.contains("getEditors(document, project)"))
        assertTrue(capture.contains("document.isWritable"))
        assertFalse(capture.contains("document.text"))
        assertFalse(capture.contains("document.charsSequence"))
        assertFalse(capture.contains("allEditors"))
        assertFalse(capture.contains("invokeAndWait"))
        assertFalse(capture.contains("schedule"))
    }

    private fun evidence(
        id: Long,
        caret: Int,
        selectionStart: Int,
        selectionEnd: Int,
    ) = EditorInputEvidence(
        eventId = id,
        focusOwnerClass = "com.intellij.openapi.editor.impl.EditorComponentImpl",
        editorFocused = true,
        documentWritable = true,
        postCaretOffset = caret,
        postSelectionStart = selectionStart,
        postSelectionEnd = selectionEnd,
        dispatchEventClass = "java.awt.event.KeyEvent",
        dispatchEventId = 400,
        dispatchModifiers = 0,
    )
}
