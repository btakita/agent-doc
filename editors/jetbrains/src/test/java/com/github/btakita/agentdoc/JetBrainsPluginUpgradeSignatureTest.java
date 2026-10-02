package com.github.btakita.agentdoc;

import org.junit.Test;

import java.util.List;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertNull;
import static org.junit.Assert.assertTrue;
import static org.junit.Assert.fail;

/**
 * `#jbunloadsig` (GH #63 layer 2): the upgrade resolves {@code DynamicPlugins} methods by name,
 * so a descriptor-type move across platform builds is absorbed instead of dying with
 * {@code NoSuchMethodError} before anything unloads.
 */
public class JetBrainsPluginUpgradeSignatureTest {
    static class BaseDescriptor {}

    static class MovedDescriptor extends BaseDescriptor {}

    static class Options {}

    /** The 2024.2-261 shape: a direct overload taking the base descriptor type. */
    public static final class LegacyPlatform {
        public String checkCanUnloadWithoutRestart(BaseDescriptor descriptor) {
            return "legacy:" + descriptor.getClass().getSimpleName();
        }

        public boolean unloadPlugin(BaseDescriptor descriptor, Options options) {
            return options != null;
        }
    }

    /** A moved shape: only the narrower descriptor type is accepted. */
    public static final class MovedPlatform {
        public String checkCanUnloadWithoutRestart(MovedDescriptor descriptor) {
            return "moved";
        }
    }

    /** Only Kotlin's {@code $default} bridge survives; the full-arity method is private. */
    public static final class BridgeOnlyPlatform {
        static int observedMask = -1;

        private String checkCanUnloadWithoutRestart(
            BaseDescriptor module,
            BaseDescriptor parent,
            Object optionalDependency,
            List<?> context,
            boolean checkImplementationDetails
        ) {
            return context == null ? "bridge:null-context" : "bridge";
        }

        static String checkCanUnloadWithoutRestart$default(
            BridgeOnlyPlatform receiver,
            BaseDescriptor module,
            BaseDescriptor parent,
            Object optionalDependency,
            List<?> context,
            boolean checkImplementationDetails,
            int mask,
            Object marker
        ) {
            observedMask = mask;
            return receiver.checkCanUnloadWithoutRestart(
                module, parent, optionalDependency, context, checkImplementationDetails
            );
        }
    }

    @Test
    public void legacyDirectOverloadIsResolvedWithTrailingArguments() {
        LegacyPlatform platform = new LegacyPlatform();
        assertEquals(
            "legacy:MovedDescriptor",
            JetBrainsPluginUpgradeAction.invokeDescriptorMethod(
                LegacyPlatform.class, platform, "checkCanUnloadWithoutRestart", new MovedDescriptor()
            )
        );
        assertEquals(
            Boolean.TRUE,
            JetBrainsPluginUpgradeAction.invokeDescriptorMethod(
                LegacyPlatform.class, platform, "unloadPlugin", new BaseDescriptor(), new Options()
            )
        );
    }

    @Test
    public void movedDescriptorTypeIsResolvedAtRuntime() {
        assertEquals(
            "moved",
            JetBrainsPluginUpgradeAction.invokeDescriptorMethod(
                MovedPlatform.class, new MovedPlatform(), "checkCanUnloadWithoutRestart",
                new MovedDescriptor()
            )
        );
    }

    @Test
    public void kotlinDefaultBridgeDefaultsEveryOptionalParameter() {
        assertEquals(
            "bridge:null-context",
            JetBrainsPluginUpgradeAction.invokeDescriptorMethod(
                BridgeOnlyPlatform.class, new BridgeOnlyPlatform(), "checkCanUnloadWithoutRestart",
                new BaseDescriptor()
            )
        );
        // Bits 1..4 mark parent, optionalDependency, context, and the boolean as defaulted.
        assertEquals(0b11110, BridgeOnlyPlatform.observedMask);
    }

    @Test
    public void unloadVerdictIsReadForBothReturnShapes() {
        // GH #67: newer platforms return a Boolean "can unload"; older ones a blocker reason.
        assertEquals(null, JetBrainsPluginUpgradeAction.unloadBlockerReason(null));
        assertEquals(null, JetBrainsPluginUpgradeAction.unloadBlockerReason(Boolean.TRUE));
        String refused = JetBrainsPluginUpgradeAction.unloadBlockerReason(Boolean.FALSE);
        assertTrue(refused, refused.startsWith("plugin cannot unload dynamically ("));
        assertTrue(refused, !refused.contains(": false"));
        assertEquals(
            "plugin cannot unload dynamically: extension point is not dynamic",
            JetBrainsPluginUpgradeAction.unloadBlockerReason("extension point is not dynamic")
        );
    }

    @Test
    public void asynchronousClassloaderRetirementStagesBeforeTouchingTheLiveGeneration() {
        assertNull(JetBrainsPluginUpgradeAction.dynamicUpgradeBlockerReason(false));
        String blocker = JetBrainsPluginUpgradeAction.dynamicUpgradeBlockerReason(true);
        assertTrue(blocker, blocker.startsWith("plugin cannot unload dynamically:"));
        assertTrue(blocker, blocker.contains("only after loading the replacement"));
        assertTrue(blocker, blocker.contains("before touching the live plugin generation"));
    }

    @Test
    public void missNamesTheDescriptorAndEverySignatureFound() {
        try {
            JetBrainsPluginUpgradeAction.invokeDescriptorMethod(
                MovedPlatform.class, new MovedPlatform(), "checkCanUnloadWithoutRestart",
                new BaseDescriptor()
            );
            fail("a descriptor no overload accepts must not be called");
        } catch (IllegalStateException miss) {
            String message = miss.getMessage();
            assertTrue(message, message.contains("no signature accepting "
                + BaseDescriptor.class.getName()));
            assertTrue(message, message.contains("on build "));
            assertTrue(message, message.contains("checkCanUnloadWithoutRestart(MovedDescriptor)"));
        }
    }
}
