package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Test

class VisualHighlighterRecencyTest {
    @Test
    fun `recent rows render a persistent rank and transient highlight`() {
        assertEquals(
            listOf("recent_rank_1", "recent_highlight_1"),
            recentPresentationKinds("recent_1", dismissed = false),
        )
        assertEquals(
            listOf("recent_rank_2", "recent_highlight_2"),
            recentPresentationKinds("recent_2", dismissed = false),
        )
        assertEquals(
            listOf("recent_rank_3", "recent_highlight_3"),
            recentPresentationKinds("recent_3", dismissed = false),
        )
    }

    @Test
    fun `caret dismissal removes only the transient highlight presentation`() {
        assertEquals(
            listOf("recent_rank_1"),
            recentPresentationKinds("recent_1", dismissed = true),
        )
    }

    @Test
    fun `ordinary native tokens keep their original presentation`() {
        assertEquals(
            listOf("tracked_id"),
            recentPresentationKinds("tracked_id", dismissed = false),
        )
    }
}
