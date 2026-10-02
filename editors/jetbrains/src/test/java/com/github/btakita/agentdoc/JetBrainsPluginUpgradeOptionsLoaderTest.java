package com.github.btakita.agentdoc;

import org.junit.Test;

import java.io.InputStream;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertSame;
import static org.junit.Assert.assertTrue;
import static org.junit.Assert.fail;

/**
 * GH #80: the update-mode unload is resolved from the signatures
 * {@code DynamicPlugins} actually exposes. Through 261 that is
 * {@code unloadPlugin(descriptor, UnloadPluginOptions)}, whose options type is taken from the
 * parameter itself; from 263 the nested options class is gone and {@code unloadPlugin} takes the
 * descriptor alone. A by-name lookup of {@code DynamicPlugins$UnloadPluginOptions} could only
 * fail there, which is what every restart-free upgrade on IU-263.6259.32 hit.
 */
public class JetBrainsPluginUpgradeOptionsLoaderTest {
    public static class Descriptor {}

    /** The 2024.2-261 shape: a nested options builder, plus a disabling one-argument default. */
    public static final class FakeDynamicPlugins {
        public static final class UnloadPluginOptions {
            public boolean disable = true;
            public boolean isUpdate = false;

            public UnloadPluginOptions() {}

            public UnloadPluginOptions withDisable(boolean value) {
                disable = value;
                return this;
            }

            public UnloadPluginOptions withUpdate(boolean value) {
                isUpdate = value;
                return this;
            }
        }

        public Object lastOptions;
        public boolean disabledByDefault;

        public boolean unloadPlugin(Descriptor descriptor, UnloadPluginOptions options) {
            lastOptions = options;
            return true;
        }

        public boolean unloadPlugin(Descriptor descriptor) {
            disabledByDefault = true;
            return true;
        }
    }

    /** The 263 shape: no {@code UnloadPluginOptions}; {@code unloadPlugin(descriptor)} only. */
    public static final class ReconfiguringDynamicPlugins {
        public int unloads;

        public boolean unloadPlugin(Descriptor descriptor) {
            unloads++;
            return true;
        }
    }

    /** An owner with no usable {@code unloadPlugin} at all. */
    public static final class OwnerWithoutUnload {
        public boolean unloadPlugin(String notADescriptor) {
            return true;
        }
    }

    @Test
    public void optionsComeFromTheUnloadSignatureInUpdateMode() throws Exception {
        // Define the owner and its nested class in a loader the test's own loader cannot see,
        // mirroring the IDE: the action's loader is not the one that defined DynamicPlugins.
        ClassLoader isolated = new IsolatedLoader(
            Descriptor.class.getName(),
            FakeDynamicPlugins.class.getName(),
            FakeDynamicPlugins.UnloadPluginOptions.class.getName()
        );
        Class<?> owner = Class.forName(FakeDynamicPlugins.class.getName(), true, isolated);
        Object platform = owner.getConstructor().newInstance();
        Object descriptor = Class.forName(Descriptor.class.getName(), true, isolated)
            .getConstructor().newInstance();
        assertSame(isolated, owner.getClassLoader());

        Object unloaded = JetBrainsPluginUpgradeAction.resolveUpdateUnload(owner, platform, descriptor).invoke();

        assertEquals(Boolean.TRUE, unloaded);
        Object options = owner.getField("lastOptions").get(platform);
        assertSame(isolated, options.getClass().getClassLoader());
        assertEquals(Boolean.FALSE, options.getClass().getField("disable").get(options));
        assertEquals(Boolean.TRUE, options.getClass().getField("isUpdate").get(options));
        assertEquals(
            "the disabling one-argument default must not be chosen while the options overload exists",
            Boolean.FALSE,
            owner.getField("disabledByDefault").get(platform)
        );
    }

    @Test
    public void aBuildWithoutUnloadPluginOptionsUnloadsThroughTheDescriptorOnlyOverload() {
        ReconfiguringDynamicPlugins platform = new ReconfiguringDynamicPlugins();

        Object unloaded = JetBrainsPluginUpgradeAction.resolveUpdateUnload(
            ReconfiguringDynamicPlugins.class, platform, new Descriptor()
        ).invoke();

        assertEquals(Boolean.TRUE, unloaded);
        assertEquals(1, platform.unloads);
    }

    @Test
    public void anUnresolvableUnloadIsAnUpgraderFailureNotARefusal() {
        try {
            JetBrainsPluginUpgradeAction.resolveUpdateUnload(
                OwnerWithoutUnload.class, new OwnerWithoutUnload(), new Descriptor()
            );
            fail("a missing unload signature must fail closed");
        } catch (IllegalStateException missing) {
            String message = missing.getMessage();
            assertTrue(message, message.startsWith(JetBrainsPluginUpgradeAction.UPGRADER_FAILED));
            assertTrue(message, !message.contains(JetBrainsPluginUpgradeAction.DYNAMIC_UNLOAD_REFUSED));
            assertTrue(message, message.contains("unloadPlugin(String)"));
        }
    }

    @Test
    public void aLinkageFailureKeepsItsClassName() {
        String described = JetBrainsPluginUpgradeAction.describeFailure(
            new NoClassDefFoundError("com/intellij/ide/plugins/DynamicPlugins$UnloadPluginOptions")
        );
        assertTrue(described, described.startsWith(JetBrainsPluginUpgradeAction.UPGRADER_FAILED));
        assertTrue(described, described.contains("java.lang.NoClassDefFoundError: com/intellij"));
        assertEquals(
            "plugin cannot unload dynamically: x",
            JetBrainsPluginUpgradeAction.describeFailure(
                new IllegalStateException("plugin cannot unload dynamically: x")
            )
        );
    }

    /** Defines the named classes itself from the test classpath bytes; delegates the rest. */
    private static final class IsolatedLoader extends ClassLoader {
        private final java.util.Set<String> owned;

        IsolatedLoader(String... owned) {
            super(JetBrainsPluginUpgradeOptionsLoaderTest.class.getClassLoader());
            this.owned = java.util.Set.of(owned);
        }

        @Override
        protected Class<?> loadClass(String name, boolean resolve) throws ClassNotFoundException {
            if (!owned.contains(name)) {
                return super.loadClass(name, resolve);
            }
            synchronized (getClassLoadingLock(name)) {
                Class<?> loaded = findLoadedClass(name);
                if (loaded == null) {
                    String resource = name.replace('.', '/') + ".class";
                    try (InputStream in = getParent().getResourceAsStream(resource)) {
                        byte[] bytes = in.readAllBytes();
                        loaded = defineClass(name, bytes, 0, bytes.length);
                    } catch (Exception failure) {
                        throw new ClassNotFoundException(name, failure);
                    }
                }
                if (resolve) {
                    resolveClass(loaded);
                }
                return loaded;
            }
        }
    }
}
