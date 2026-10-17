package com.github.btakita.agentdoc

import java.nio.file.Files
import java.nio.file.Path
import org.junit.Assert.assertTrue
import org.junit.Test

class FocusedExtractCoordinationTest {
    @Test
    fun `coordinated extract freezes registration and resumes before restoring writes`() {
        val watcher =
            Files.readString(Path.of("src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"))
        val manager =
            Files.readString(Path.of("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"))

        assertTrue(watcher.contains("coordinated_extract_v1"))
        assertTrue(watcher.contains("FreezeForExtract(\"freeze_for_extract\")"))
        assertTrue(watcher.contains("ResumeAfterExtract(\"resume_after_extract\")"))
        assertTrue(manager.contains("extractFrozenPaths.contains(filePath)"))
        assertTrue(manager.contains("focused-extract freeze still pending"))

        val freeze = watcher.substringAfter("private fun freezeForExtract(")
            .substringBefore("private fun resumeAfterExtract(")
        assertTrue(
            freeze.indexOf("document.setReadOnly(true)") <
                freeze.indexOf("freezeOpenDocumentForExtract"),
        )

        val resume = watcher.substringAfter("private fun resumeAfterExtract(")
            .substringBefore("private fun refreshContentViaDocument(")
        assertTrue(
            resume.indexOf("reloadFromDisk(document)") <
                resume.indexOf("resumeOpenDocumentAfterExtract"),
        )
        assertTrue(
            resume.indexOf("resumeOpenDocumentAfterExtract") <
                resume.lastIndexOf("setReadOnly(!frozen.wasWritable)"),
        )
    }
}
