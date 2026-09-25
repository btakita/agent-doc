package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import java.nio.file.Files
import java.nio.file.Paths
import org.junit.Test

class PluginLifecycleListenerTest {

    @Test
    fun `startup does not run automatic resync audit`() {
        val source = Files.readString(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PluginLifecycleListener.kt")
                .takeIf { Files.exists(it) }
                ?: Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PluginLifecycleListener.kt")
        )

        assertFalse(source.contains("agent-doc\", \"resync"))
        assertFalse(source.contains("agent-doc\", \"resync\", \"--fix"))
        assertFalse(source.contains("agent-doc-resync"))
        // `#ctrlkillreregister` Tier 3: startup asks which registrations are actually
        // stranded rather than re-registering every open document. See
        // [PeerReplicaPullTest] for why the blind sweep is the wrong startup shape.
        assertTrue(
            source.contains(
                "CrdtReplicaManager.pullMissingReplicas(project, \"plugin-startup\")"
            )
        )
        assertFalse(source.contains("openFile("))
        assertTrue(source.contains("TmuxPaneFocusSync.install(project)"))

        val focusSyncSource = Files.readString(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/TmuxPaneFocusSync.kt")
                .takeIf { Files.exists(it) }
                ?: Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/TmuxPaneFocusSync.kt"),
        )
        assertEquals(
            "only explicit install may construct the reverse-focus poller",
            1,
            Regex("""instances\.computeIfAbsent\(project\)""").findAll(focusSyncSource).count(),
        )
        assertTrue(focusSyncSource.contains("manager.openFile(file, false, true)"))
        assertFalse(focusSyncSource.contains("manager.openFile(file, true, true)"))
    }

    @Test
    fun `plugin package supports restart-free dynamic upgrades`() {
        val pluginXml = Files.readString(
            Paths.get("src/main/resources/META-INF/plugin.xml")
                .takeIf { Files.exists(it) }
                ?: Paths.get("editors/jetbrains/src/main/resources/META-INF/plugin.xml")
        )

        assertTrue(pluginXml.contains("<idea-plugin>"))
        assertFalse(pluginXml.contains("require-restart"))
        assertTrue(pluginXml.contains("PluginUnloadCleanupService"))
        assertTrue(pluginXml.contains("ProjectPluginLifecycleService"))

        val source = Files.readString(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PluginLifecycleListener.kt")
                .takeIf { Files.exists(it) }
                ?: Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PluginLifecycleListener.kt")
        )
        assertTrue(source.contains("class PluginUnloadCleanupService : Disposable"))
        assertTrue(source.contains("class ProjectPluginLifecycleService"))
        assertTrue(source.contains("connect(lifecycle)"))
        assertFalse(
            "programmatic listeners must not outlive their plugin generation",
            source.contains(".connect(project)"),
        )
        assertTrue(source.contains("addDocumentListener(TypingTracker, lifecycle)"))
        assertTrue(source.contains("ReliableSyncLivenessListener.install(project, lifecycle)"))
        assertTrue(source.contains("ProjectManager.getInstance().openProjects"))
        assertTrue(source.contains("initializeOpenProjectsAfterDynamicLoad"))
        assertTrue(source.contains("fun disposeOpenProjectsForDynamicUnload(): Int"))
        assertTrue(source.contains("projects.forEach(::disposeProjectResources)"))
        assertTrue(source.contains("ensureOpenDocumentReplicasAndWait"))
        assertTrue(source.contains("beginInitialization()"))
        assertTrue(source.contains("disposeProjectResources"))

        val turnStateRefresher = Files.readString(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/TurnStateBannerRefresher.kt")
                .takeIf { Files.exists(it) }
                ?: Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/TurnStateBannerRefresher.kt"),
        )
        assertFalse(
            "light services remain cached by class name across a dynamic plugin replacement",
            turnStateRefresher.contains("@Service(Service.Level.PROJECT)"),
        )
        assertFalse(turnStateRefresher.contains("project.service()"))
        assertTrue(turnStateRefresher.contains("instances.computeIfAbsent(project"))
        assertTrue(turnStateRefresher.contains("fun disposeProject(project: Project)"))
        assertTrue(source.contains("TurnStateBannerRefresher.disposeProject(project)"))

        val upgradeAction = Files.readString(
            Paths.get("src/main/java/com/github/btakita/agentdoc/JetBrainsPluginUpgradeAction.java")
                .takeIf { Files.exists(it) }
                ?: Paths.get("editors/jetbrains/src/main/java/com/github/btakita/agentdoc/JetBrainsPluginUpgradeAction.java"),
        )
        assertTrue(upgradeAction.contains("initializeOpenProjectsAfterDynamicLoad"))
        assertTrue(upgradeAction.contains("cleanupOutgoingGeneration(current)"))
        assertTrue(upgradeAction.contains("disposeOpenProjectsForDynamicUnload"))
        assertTrue(upgradeAction.contains("disposeProjectResources$"))
        assertTrue(
            "outgoing document listeners must stop before IntelliJ unloads their descriptor",
            upgradeAction.indexOf("cleanupOutgoingGeneration(current)") <
                upgradeAction.indexOf("unloadPlugin(current, updateOptions)"),
        )
        assertTrue(upgradeAction.contains("documents="))
        assertTrue(upgradeAction.contains(".withDisable(false)"))
        assertTrue(upgradeAction.contains(".withUpdate(true)"))
        assertFalse(upgradeAction.contains("unloadPlugin(current)"))
        assertTrue(upgradeAction.contains("actual == current"))
        assertTrue(upgradeAction.contains("actual.getPluginClassLoader() == current.getPluginClassLoader()"))

        val replicaManager = Files.readString(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt")
                .takeIf { Files.exists(it) }
                ?: Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
        )
        assertTrue(replicaManager.contains("DYNAMIC_PLUGIN_ATTACH_RECEIPT_TIMEOUT_MS"))
        assertTrue(replicaManager.contains("manager.forwarders[filePath]?.attached == true"))

        val livenessListener = Files.readString(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/ReliableSyncLivenessListener.kt")
                .takeIf { Files.exists(it) }
                ?: Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/ReliableSyncLivenessListener.kt"),
        )
        assertTrue(livenessListener.contains("fun install(project: Project, lifecycle: Disposable)"))
        assertTrue(livenessListener.contains("instances.computeIfAbsent(project)"))
        assertTrue(livenessListener.contains(".connect(lifecycle)"))
        assertFalse(
            "the liveness publisher must be rebuilt explicitly for surviving projects",
            pluginXml.contains("class=\"com.github.btakita.agentdoc.ReliableSyncLivenessListener\""),
        )
    }

    @Test
    fun `IDE activation republishes the settled editor surface`() {
        val source =
            Files.readString(
                Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PluginLifecycleListener.kt")
                    .takeIf { Files.exists(it) }
                    ?: Paths.get(
                        "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PluginLifecycleListener.kt",
                    ),
            )

        assertTrue(source.contains("ApplicationActivationListener.TOPIC"))
        assertTrue(source.contains("override fun applicationActivated(ideFrame: IdeFrame)"))
        assertTrue(source.contains("editorTabSync.onIdeActivated(project)"))
    }
}
