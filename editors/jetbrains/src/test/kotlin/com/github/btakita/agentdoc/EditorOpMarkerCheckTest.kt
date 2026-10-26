package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** `#componentownclose`: the Kotlin side only maps native's verdict and dedupes warnings. */
class EditorOpMarkerCheckTest {
    @Test
    fun `native return codes map to the marker check verdict`() {
        assertEquals(EditorOpMarkerCheck.UNCHANGED, EditorOpMarkerCheck.fromCode(0))
        assertEquals(EditorOpMarkerCheck.MARKERS_REMOVED, EditorOpMarkerCheck.fromCode(1))
        assertEquals(EditorOpMarkerCheck.MARKERS_REPAIRED, EditorOpMarkerCheck.fromCode(2))
        assertEquals(EditorOpMarkerCheck.UNAVAILABLE, EditorOpMarkerCheck.fromCode(-1))
        assertEquals(EditorOpMarkerCheck.UNAVAILABLE, EditorOpMarkerCheck.fromCode(7))

        assertTrue(EditorOpMarkerCheck.MARKERS_REMOVED.changesDiagnostic)
        assertTrue(EditorOpMarkerCheck.MARKERS_REPAIRED.changesDiagnostic)
        assertFalse(EditorOpMarkerCheck.UNCHANGED.changesDiagnostic)
        assertFalse(EditorOpMarkerCheck.UNAVAILABLE.changesDiagnostic)
    }

    @Test
    fun `a marker break warns once until the markers are repaired`() {
        val warnings = EditorOpMarkerWarnings()
        val doc = "/project/tasks/bugs.md"
        val other = "/project/tasks/plan.md"

        assertTrue(warnings.observe(doc, EditorOpMarkerCheck.MARKERS_REMOVED))
        // Native re-reports the same break while the operator types on: no second warning.
        assertFalse(warnings.observe(doc, EditorOpMarkerCheck.MARKERS_REMOVED))
        assertFalse(warnings.observe(doc, EditorOpMarkerCheck.UNCHANGED))
        // Another document has its own warning.
        assertTrue(warnings.observe(other, EditorOpMarkerCheck.MARKERS_REMOVED))

        // The repair never warns, and re-arms the next break.
        assertFalse(warnings.observe(doc, EditorOpMarkerCheck.MARKERS_REPAIRED))
        assertTrue(warnings.observe(doc, EditorOpMarkerCheck.MARKERS_REMOVED))
        assertFalse(warnings.observe(doc, EditorOpMarkerCheck.UNAVAILABLE))
    }
}
