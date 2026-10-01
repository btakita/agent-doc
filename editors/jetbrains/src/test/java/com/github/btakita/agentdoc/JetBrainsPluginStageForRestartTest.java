package com.github.btakita.agentdoc;

import org.junit.Test;

import java.lang.reflect.Method;
import java.nio.file.Path;

import static org.junit.Assert.assertArrayEquals;
import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertNull;

/** `#jbstageonfail`: staging resolves `installAfterRestart` across platform signature moves. */
public class JetBrainsPluginStageForRestartTest {
    public static final class Descriptor {}

    public static final class DescriptorFirst {
        public static boolean installAfterRestart(Descriptor descriptor, Path source, Path existing, boolean delete) {
            return true;
        }
    }

    public static final class DescriptorLast {
        public static void installAfterRestart(Path source, boolean delete, Path existing, Descriptor descriptor) {}
    }

    public static final class NoMatch {
        public static void installAfterRestart(String unrelated, Path source) {}

        public void installAfterRestart(Descriptor descriptor, Path source, Path existing) {}
    }

    private static final Path SOURCE = Path.of("/tmp/plugins/agent-doc-jetbrains-0.2.456.zip");
    private static final Path EXISTING = Path.of("/plugins/agent-doc-jetbrains");

    @Test
    public void descriptorFirstSignatureGetsSourceBeforeExisting() {
        Descriptor descriptor = new Descriptor();
        Method install = JetBrainsPluginUpgradeAction.findInstallAfterRestart(DescriptorFirst.class, descriptor);
        assertNotNull(install);
        assertArrayEquals(
            new Object[] {descriptor, SOURCE, EXISTING, true},
            JetBrainsPluginUpgradeAction.installAfterRestartArguments(install, descriptor, SOURCE, EXISTING)
        );
    }

    @Test
    public void descriptorLastSignatureGetsSourceBeforeExisting() {
        Descriptor descriptor = new Descriptor();
        Method install = JetBrainsPluginUpgradeAction.findInstallAfterRestart(DescriptorLast.class, descriptor);
        assertNotNull(install);
        Object[] args = JetBrainsPluginUpgradeAction.installAfterRestartArguments(install, descriptor, SOURCE, EXISTING);
        assertEquals(SOURCE, args[0]);
        assertEquals(true, args[1]);
        assertEquals(EXISTING, args[2]);
        assertEquals(descriptor, args[3]);
    }

    @Test
    public void instanceAndForeignOverloadsAreNotStagingCandidates() {
        assertNull(JetBrainsPluginUpgradeAction.findInstallAfterRestart(NoMatch.class, new Descriptor()));
    }
}
