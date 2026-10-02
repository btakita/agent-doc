package com.github.btakita.agentdoc;

import org.junit.Test;

import java.util.ArrayList;
import java.util.List;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertTrue;
import static org.junit.Assert.fail;

/**
 * GH #94: an upgrade that aborts must not leave the live plugin generation with its replica
 * transport torn down. The outgoing generation releases its open projects only once the unload
 * is known to be callable, and a failure after that release rebuilds them from the still-loaded
 * generation.
 */
public class JetBrainsPluginUnloadRestoreTest {
    /** Records the order the upgrade drives the outgoing generation in. */
    private static final class RecordingGeneration implements JetBrainsPluginUpgradeAction.OutgoingGeneration {
        final List<String> calls = new ArrayList<>();
        Object verdict = Boolean.TRUE;
        RuntimeException resolveFailure;
        Object unloadResult = Boolean.TRUE;

        @Override
        public Object unloadVerdict() {
            calls.add("verdict");
            return verdict;
        }

        @Override
        public JetBrainsPluginUpgradeAction.UnloadCall resolveUnload() {
            calls.add("resolve");
            if (resolveFailure != null) {
                throw resolveFailure;
            }
            return () -> {
                calls.add("unload");
                return unloadResult;
            };
        }

        @Override
        public int releaseOpenProjects() {
            calls.add("release");
            return 2;
        }
    }

    @Test
    public void anUnresolvableUnloadAbortsBeforeTheReplicaTransportIsReleased() {
        // The GH #80 -> #94 chain: on IU-263 the options lookup failed AFTER the release.
        RecordingGeneration generation = new RecordingGeneration();
        generation.resolveFailure = new IllegalStateException(
            JetBrainsPluginUpgradeAction.UPGRADER_FAILED + ": no unloadPlugin"
        );
        try {
            JetBrainsPluginUpgradeAction.unloadOutgoingGeneration(generation);
            fail("an unresolvable unload must abort the upgrade");
        } catch (IllegalStateException expected) {
            assertTrue(expected.getMessage().startsWith(JetBrainsPluginUpgradeAction.UPGRADER_FAILED));
        }
        assertEquals(List.of("verdict", "resolve"), generation.calls);
    }

    @Test
    public void aPlatformBlockerAbortsBeforeAnythingIsReleased() {
        RecordingGeneration generation = new RecordingGeneration();
        generation.verdict = Boolean.FALSE;
        try {
            JetBrainsPluginUpgradeAction.unloadOutgoingGeneration(generation);
            fail("a blocker must abort the upgrade");
        } catch (IllegalStateException expected) {
            assertTrue(expected.getMessage().startsWith(JetBrainsPluginUpgradeAction.DYNAMIC_UNLOAD_REFUSED));
        }
        assertEquals(List.of("verdict"), generation.calls);
    }

    @Test
    public void theReleaseRunsAfterResolutionAndBeforeTheUnload() {
        RecordingGeneration generation = new RecordingGeneration();

        assertEquals(2, JetBrainsPluginUpgradeAction.unloadOutgoingGeneration(generation));
        assertEquals(List.of("verdict", "resolve", "release", "unload"), generation.calls);
    }

    @Test
    public void aRefusedUnloadAfterTheReleaseIsReportedAsARefusal() {
        RecordingGeneration generation = new RecordingGeneration();
        generation.unloadResult = Boolean.FALSE;
        try {
            JetBrainsPluginUpgradeAction.unloadOutgoingGeneration(generation);
            fail("a refused unload must abort the upgrade");
        } catch (IllegalStateException expected) {
            assertTrue(expected.getMessage().startsWith(JetBrainsPluginUpgradeAction.DYNAMIC_UNLOAD_REFUSED));
        }
        assertEquals(List.of("verdict", "resolve", "release", "unload"), generation.calls);
    }

    @Test
    public void anAbortAfterTheReleaseRebuildsTheStillLoadedGeneration() {
        List<String> reattached = new ArrayList<>();

        String suffix = JetBrainsPluginUpgradeAction.recoverAbortedUnload(
            true,
            () -> true,
            () -> {
                reattached.add("reattach");
                return "documents=3/3";
            }
        );

        assertEquals(List.of("reattach"), reattached);
        assertTrue(suffix, suffix.contains("restored the live plugin generation's replica transport"));
        assertTrue(suffix, suffix.endsWith("documents=3/3"));
    }

    @Test
    public void noReleaseMeansNoRestore() {
        String suffix = JetBrainsPluginUpgradeAction.recoverAbortedUnload(
            false,
            () -> {
                throw new AssertionError("must not consult the loaded state");
            },
            () -> {
                throw new AssertionError("must not reattach a generation that never released");
            }
        );

        assertEquals("", suffix);
    }

    @Test
    public void anAlreadyUnloadedGenerationNamesTheRestartInsteadOfReattaching() {
        String suffix = JetBrainsPluginUpgradeAction.recoverAbortedUnload(
            true,
            () -> false,
            () -> {
                throw new AssertionError("an unloaded generation's classes must not be driven");
            }
        );

        assertTrue(suffix, suffix.contains("already unloaded"));
        assertTrue(suffix, suffix.contains("restarts"));
    }

    @Test
    public void aFailedRestoreIsReportedNotRaised() {
        String suffix = JetBrainsPluginUpgradeAction.recoverAbortedUnload(
            true,
            () -> true,
            () -> {
                throw new IllegalStateException("controller unreachable");
            }
        );

        assertTrue(suffix, suffix.contains("reattach_error=controller unreachable"));
    }
}
