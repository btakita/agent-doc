package com.github.btakita.agentdoc

import io.github.lazily.ThreadSafeContext
import java.util.concurrent.atomic.AtomicInteger
import org.junit.Assert.assertEquals
import org.junit.Test

class TemplateValidationPlaneTest {
    @Test
    fun `unchanged revision is memoized and changed revision recomputes`() {
        val normalizations = AtomicInteger()
        val plane =
            TemplateValidationPlane(
                ThreadSafeContext(),
                normalize = { text ->
                    normalizations.incrementAndGet()
                    NativePatching.TemplateStructureNormalization(
                        text.takeUnless { it == "invalid" },
                        if (text == "invalid") "rule=duplicate_exchange_boundary offset=17" else null,
                    )
                },
                hash = { text -> "hash:$text" },
            )

        assertEquals(
            TemplateStructureProjectionState.Exact,
            plane.publish("session.md", TemplateValidationPlane.Lane.Editor, "valid").state,
        )
        assertEquals(
            TemplateStructureProjectionState.Exact,
            plane.publish("session.md", TemplateValidationPlane.Lane.Editor, "valid").state,
        )
        assertEquals(1, normalizations.get())

        val changed =
            plane.publish("session.md", TemplateValidationPlane.Lane.Editor, "invalid")
        assertEquals(TemplateStructureProjectionState.Invalid, changed.state)
        assertEquals("hash:invalid", changed.revisionHash)
        assertEquals("rule=duplicate_exchange_boundary offset=17", changed.diagnostic)
        assertEquals(2, normalizations.get())
    }

    @Test
    fun `editor and remote candidate validation invalidate independently`() {
        val normalizations = AtomicInteger()
        val plane =
            TemplateValidationPlane(
                ThreadSafeContext(),
                normalize = { text ->
                    normalizations.incrementAndGet()
                    NativePatching.TemplateStructureNormalization(text, null)
                },
                hash = { text -> "hash:$text" },
            )

        plane.publish("session.md", TemplateValidationPlane.Lane.Editor, "editor")
        plane.publish("session.md", TemplateValidationPlane.Lane.RemoteCandidate, "remote")
        plane.publish("session.md", TemplateValidationPlane.Lane.Editor, "editor")

        assertEquals(2, normalizations.get())
    }
}
