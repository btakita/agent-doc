package com.github.btakita.agentdoc

import com.intellij.notification.NotificationType
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.Paths
import kotlin.io.path.extension
import kotlin.io.path.isRegularFile
import kotlin.io.path.name
import kotlin.io.path.readText

/** GH #434: every operator-facing toast leaves a matching idea.log line. */
class AgentDocNoticesTest {
    @Test
    fun `a shown line names the source, type, title, and the plain text the operator read`() {
        val line =
            AgentDocNotices.shownLine(
                source = "TurnStateBannerRefresher.buildInputRequiredNotification",
                type = NotificationType.WARNING,
                title = "Agent input required for plan.md",
                content = TerminalUtil.notificationHtml("Open the \"Agent Doc\" terminal <now>\n& answer."),
                project = "agent-loop",
            )
        assertEquals(
            "[notify] shown source=TurnStateBannerRefresher.buildInputRequiredNotification type=WARNING " +
                "project=\"agent-loop\" title=\"Agent input required for plan.md\" " +
                "content=\"Open the \\\"Agent Doc\\\" terminal <now>\\n& answer.\"",
            line,
        )
        assertTrue("grep `input required` must find the toast: $line", line.contains("input required"))
        assertTrue("one log line", !line.contains('\n'))
    }

    @Test
    fun `an expired line pairs with the shown line`() {
        assertEquals(
            "[notify] expired source=s title=\"t\" content=\"a\\nb\"",
            AgentDocNotices.expiredLine("s", "t", "a<br>b"),
        )
    }

    @Test
    fun `long content is capped with its length`() {
        val text = AgentDocNotices.plainText("x".repeat(AgentDocNotices.MAX_LOGGED_CONTENT + 5))
        assertTrue(text, text.endsWith("...(${AgentDocNotices.MAX_LOGGED_CONTENT + 5} chars)"))
    }

    /**
     * The conformance half: a bare `.notify(` on a notification shows a toast the log never sees.
     * Every Kotlin main source in both packages goes through [AgentDocNotices.show]; the child-first
     * Java upgrade bridge cannot reach the live generation's helper, so it logs its own line.
     */
    @Test
    fun `no main source shows a notification without logging it`() {
        val roots =
            listOf(
                Paths.get("src/main"),
                Paths.get("../jetbrains-262/backend/src/main"),
                Paths.get("../jetbrains-262/frontend/src/main"),
                Paths.get("../jetbrains-262/frontend-impl/src/main"),
                Paths.get("../jetbrains-262/shared/src/main"),
            ).filter(Files::isDirectory)
        assertTrue("the classic sources are on the scan path", roots.first() == Paths.get("src/main"))
        val bare = Regex("""\.notify\(""")
        val offenders = mutableListOf<String>()
        var scanned = 0
        for (root in roots) {
            Files.walk(root).use { paths ->
                paths
                    .filter { it.isRegularFile() && it.extension in setOf("kt", "java") }
                    .forEach { path: Path ->
                        scanned++
                        if (path.name == "AgentDocNotices.kt") return@forEach
                        val source = path.readText()
                        source.lineSequence().forEachIndexed { index, line ->
                            if (!bare.containsMatchIn(line)) return@forEachIndexed
                            val loggedBridge =
                                path.extension == "java" &&
                                    source.lineSequence().drop(maxOf(0, index - 8)).take(8)
                                        .any { it.contains("\"[notify] shown source=") }
                            if (!loggedBridge) offenders += "$path:${index + 1}: ${line.trim()}"
                        }
                    }
            }
        }
        assertTrue("scanned no sources", scanned > 50)
        assertTrue(
            "show notifications through AgentDocNotices.show so idea.log records them:\n" +
                offenders.joinToString("\n"),
            offenders.isEmpty(),
        )
    }

    @Test
    fun `the input-required toast is raised and retracted through the logged helper`() {
        val refresher =
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/TurnStateBannerRefresher.kt").readText()
        assertTrue(
            refresher.contains(
                "AgentDocNotices.show(project, notification, inputRequiredNoticeSource(reason))",
            ),
        )
        assertTrue(refresher.contains("AgentDocNotices.expire(it, \"TurnStateBannerRefresher.inputRequired\")"))
    }
}
