package com.github.btakita.agentdoc;

import com.intellij.ide.startup.StartupActionScriptManager;
import org.junit.Rule;
import org.junit.Test;
import org.junit.rules.TemporaryFolder;

import java.io.OutputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import java.util.concurrent.TimeUnit;
import java.util.zip.ZipEntry;
import java.util.zip.ZipOutputStream;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertNotEquals;
import static org.junit.Assert.assertTrue;
import static org.junit.Assert.fail;

/**
 * GH #115: a staged restart upgrade must never leave the IDE's pending-install script able to
 * delete the plugin without installing its replacement. Exercised against the platform's real
 * {@link StartupActionScriptManager} load/save, so the test is independent of the script format.
 */
public class JetBrainsPluginUpgradeStagingTest {
    @Rule
    public TemporaryFolder folder = new TemporaryFolder();

    private Path pluginsDir() throws Exception {
        return Files.createDirectories(folder.getRoot().toPath().resolve("data/IntelliJIdea2026.3"));
    }

    private Path tempDir() throws Exception {
        return Files.createDirectories(folder.getRoot().toPath().resolve("cache/IntelliJIdea2026.3/plugins"));
    }

    private Path packageZip(String name) throws Exception {
        Path zip = folder.getRoot().toPath().resolve(name);
        try (ZipOutputStream out = new ZipOutputStream(Files.newOutputStream(zip))) {
            out.putNextEntry(new ZipEntry("agent-doc-jetbrains/lib/agent-doc-jetbrains-0.2.481.jar"));
            out.write(new byte[] {1, 2, 3});
            out.closeEntry();
        }
        return zip;
    }

    private Path stage(Path script, Path pluginsDir, Path tempDir) throws Exception {
        Path staged = JetBrainsPluginUpgradeAction.copyVerifiedStagedArchive(
            packageZip("launcher-" + System.nanoTime() + ".zip"), tempDir, "0.2.481"
        );
        JetBrainsPluginUpgradeAction.replaceStagingInActionScript(
            StartupActionScriptManager.class,
            script,
            pluginsDir.resolve("agent-doc-jetbrains"),
            pluginsDir,
            staged,
            () -> {
                fail("the platform's command classes are constructible on this build");
                return null;
            }
        );
        return staged;
    }

    private static List<String[]> described(Path script) throws Exception {
        List<String[]> commands = new ArrayList<>();
        for (Object command : StartupActionScriptManager.loadActionScript(script)) {
            commands.add(JetBrainsPluginUpgradeAction.describeCommand(command));
        }
        return commands;
    }

    /** Every unzip's package exists, and nothing deletes it before the unzip runs. */
    private static void assertNoUnzipCanBeStranded(List<String[]> commands) {
        for (int index = 0; index < commands.size(); index++) {
            String[] command = commands.get(index);
            if (!command[0].equals("UnzipCommand")) {
                continue;
            }
            assertTrue("unzip source must exist: " + command[1], Files.isRegularFile(Path.of(command[1])));
            for (int earlier = 0; earlier < index; earlier++) {
                String[] before = commands.get(earlier);
                assertFalse(
                    "a delete ahead of the unzip removes its package: " + before[1],
                    before[0].equals("DeleteCommand") && before[1].equals(command[1])
                );
            }
        }
    }

    @Test
    public void duplicateStagingLeavesOneDeleteUnzipBlock() throws Exception {
        Path pluginsDir = pluginsDir();
        Path tempDir = tempDir();
        Path script = tempDir.resolve("action.script");
        // A foreign plugin's pending update must survive the rewrite untouched.
        StartupActionScriptManager.saveActionScript(
            List.of(new StartupActionScriptManager.DeleteCommand(pluginsDir.resolve("other-plugin"))),
            script
        );

        Path first = stage(script, pluginsDir, tempDir);
        Path second = stage(script, pluginsDir, tempDir);

        assertNotEquals("each staging gets its own package name", first, second);
        List<String[]> commands = described(script);
        assertEquals(4, commands.size());
        assertEquals("DeleteCommand", commands.get(0)[0]);
        assertEquals(pluginsDir.resolve("other-plugin").toString(), commands.get(0)[1]);
        assertEquals("DeleteCommand", commands.get(1)[0]);
        assertEquals(pluginsDir.resolve("agent-doc-jetbrains").toString(), commands.get(1)[1]);
        assertEquals("UnzipCommand", commands.get(2)[0]);
        assertEquals(second.toString(), commands.get(2)[1]);
        assertEquals(pluginsDir.toString(), commands.get(2)[2]);
        assertEquals("DeleteCommand", commands.get(3)[0]);
        assertEquals(second.toString(), commands.get(3)[1]);
        assertNoUnzipCanBeStranded(commands);
        assertFalse("the superseded package is cleaned up", Files.exists(first));
        assertTrue(Files.isRegularFile(second));
    }

    /**
     * The GH #115 script: an older agent-doc staged the fixed `agent-doc-jetbrains-<v>.zip` name.
     * Re-staging must replace that block, not append behind it.
     */
    @Test
    public void secondStagingNeverRemovesAZipTheScriptStillNeeds() throws Exception {
        Path pluginsDir = pluginsDir();
        Path tempDir = tempDir();
        Path script = tempDir.resolve("action.script");
        Path legacy = Files.copy(packageZip("legacy.zip"), tempDir.resolve("agent-doc-jetbrains-0.2.481.zip"));
        Path pluginDir = pluginsDir.resolve("agent-doc-jetbrains");
        StartupActionScriptManager.saveActionScript(
            List.of(
                new StartupActionScriptManager.DeleteCommand(pluginDir),
                new StartupActionScriptManager.DeleteCommand(pluginDir),
                new StartupActionScriptManager.UnzipCommand(legacy, pluginsDir),
                new StartupActionScriptManager.DeleteCommand(legacy)
            ),
            script
        );

        Path staged = stage(script, pluginsDir, tempDir);

        List<String[]> commands = described(script);
        assertEquals(3, commands.size());
        long pluginDeletes = commands.stream()
            .filter(command -> command[0].equals("DeleteCommand") && command[1].equals(pluginDir.toString()))
            .count();
        assertEquals("exactly one delete of the plugin directory", 1, pluginDeletes);
        assertEquals(staged.toString(), commands.get(1)[1]);
        assertNoUnzipCanBeStranded(commands);
    }

    @Test
    public void concurrentStagingsSerializeIntoOneBlock() throws Exception {
        Path pluginsDir = pluginsDir();
        Path tempDir = tempDir();
        Path script = tempDir.resolve("action.script");
        int stagings = 6;
        ExecutorService pool = Executors.newFixedThreadPool(stagings);
        CountDownLatch start = new CountDownLatch(1);
        List<Future<Path>> results = new ArrayList<>();
        try {
            for (int index = 0; index < stagings; index++) {
                results.add(pool.submit(() -> {
                    start.await();
                    return stage(script, pluginsDir, tempDir);
                }));
            }
            start.countDown();
            for (Future<Path> result : results) {
                result.get(30, TimeUnit.SECONDS);
            }
        } finally {
            pool.shutdownNow();
        }
        List<String[]> commands = described(script);
        assertEquals(3, commands.size());
        assertEquals("UnzipCommand", commands.get(1)[0]);
        assertNoUnzipCanBeStranded(commands);
        try (var listing = Files.list(tempDir)) {
            long packages = listing
                .filter(path -> JetBrainsPluginUpgradeAction.isStagedArchive(path.toString()))
                .count();
            assertEquals("only the surviving staging keeps a package", 1, packages);
        }
    }

    @Test
    public void anArchiveWithoutThePluginJarIsNeverStaged() throws Exception {
        Path tempDir = tempDir();
        Path empty = folder.getRoot().toPath().resolve("empty.zip");
        try (OutputStream raw = Files.newOutputStream(empty); ZipOutputStream out = new ZipOutputStream(raw)) {
            out.putNextEntry(new ZipEntry("README"));
            out.closeEntry();
        }
        try {
            JetBrainsPluginUpgradeAction.copyVerifiedStagedArchive(empty, tempDir, "0.2.481");
            fail("an archive with no plugin jar must be refused");
        } catch (IllegalStateException expected) {
            assertTrue(expected.getMessage(), expected.getMessage().contains("refusing to stage"));
        }
        try (var listing = Files.list(tempDir)) {
            assertEquals("no partial or staged copy is left behind", 0, listing.count());
        }
    }

    @Test
    public void stagedArchiveNamesAreUniqueAndRecognized() {
        String name = JetBrainsPluginUpgradeAction.stagedArchiveName("0.2.481", "abc123");
        assertEquals("agent-doc-jetbrains-0.2.481+abc123.zip", name);
        assertTrue(JetBrainsPluginUpgradeAction.isStagedArchive("/c/" + name));
        assertTrue(JetBrainsPluginUpgradeAction.isStagedArchive("/c/agent-doc-jetbrains-0.2.481.zip"));
        assertFalse(JetBrainsPluginUpgradeAction.isStagedArchive("/c/other-plugin-1.0.zip"));
        assertFalse(JetBrainsPluginUpgradeAction.isStagedArchive("/d/agent-doc-jetbrains"));
    }
}
