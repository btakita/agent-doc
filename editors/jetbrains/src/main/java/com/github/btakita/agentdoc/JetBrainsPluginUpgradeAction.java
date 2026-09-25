package com.github.btakita.agentdoc;

import com.intellij.ide.plugins.DynamicPlugins;
import com.intellij.ide.plugins.IdeaPluginDescriptor;
import com.intellij.ide.plugins.IdeaPluginDescriptorImpl;
import com.intellij.ide.plugins.PluginInstaller;
import com.intellij.ide.plugins.PluginManagerCore;
import com.intellij.openapi.application.ApplicationManager;
import com.intellij.openapi.extensions.PluginDescriptor;
import com.intellij.openapi.extensions.PluginId;

import java.lang.reflect.Method;
import java.nio.file.Path;
import java.util.concurrent.atomic.AtomicReference;

/** Freshly loaded implementation of one JetBrains plugin package replacement. */
public final class JetBrainsPluginUpgradeAction {
    private static final String PLUGIN_ID = "com.github.btakita.agent-doc";

    private JetBrainsPluginUpgradeAction() {}

    public static String run(String archiveValue, String pluginsDirValue, String expectedVersion) {
        AtomicReference<String> result = new AtomicReference<>();
        AtomicReference<Throwable> failure = new AtomicReference<>();
        ApplicationManager.getApplication().invokeAndWait(() -> {
            try {
                result.set(runOnEdt(archiveValue, pluginsDirValue, expectedVersion));
            } catch (Throwable caught) {
                failure.set(caught);
            }
        });
        if (failure.get() != null) {
            throw new IllegalStateException(failure.get().getMessage(), failure.get());
        }
        return result.get();
    }

    private static String runOnEdt(String archiveValue, String pluginsDirValue, String expectedVersion) {
        Path archive = Path.of(archiveValue).toAbsolutePath().normalize();
        Path pluginsDir = Path.of(pluginsDirValue).toAbsolutePath().normalize();

        IdeaPluginDescriptor descriptor = PluginManagerCore.getPlugin(PluginId.getId(PLUGIN_ID));
        if (!(descriptor instanceof IdeaPluginDescriptorImpl current)) {
            return "skip:plugin-not-loaded";
        }
        Path expectedRoot = pluginsDir.resolve("agent-doc-jetbrains").toAbsolutePath().normalize();
        if (!sameInstallRoot(current.getPluginPath(), expectedRoot)) {
            return "skip:different-plugin-root:" + current.getPluginPath();
        }

        if (isLoaded(current)) {
            String unloadBlocker = DynamicPlugins.INSTANCE.checkCanUnloadWithoutRestart(current);
            if (unloadBlocker != null) {
                throw new IllegalStateException("plugin cannot unload dynamically: " + unloadBlocker);
            }
            if (!DynamicPlugins.INSTANCE.unloadPlugin(current)) {
                throw new IllegalStateException("JetBrains refused to unload the current plugin generation");
            }
        }

        boolean loaded = PluginInstaller.installAndLoadDynamicPlugin(archive, current);
        IdeaPluginDescriptor actualDescriptor = PluginManagerCore.getPlugin(PluginId.getId(PLUGIN_ID));
        if (!loaded || actualDescriptor == null || !expectedVersion.equals(actualDescriptor.getVersion())) {
            String actual = actualDescriptor == null ? "missing" : actualDescriptor.getVersion();
            throw new IllegalStateException(
                "dynamic install returned " + loaded + "; expected " + expectedVersion + ", loaded " + actual
            );
        }
        return "ok:" + expectedVersion;
    }

    static boolean sameInstallRoot(Path actual, Path expected) {
        return actual.toAbsolutePath().normalize().equals(expected.toAbsolutePath().normalize());
    }

    private static boolean isLoaded(IdeaPluginDescriptorImpl descriptor) {
        try {
            Method method = PluginManagerCore.class.getMethod("isLoaded", PluginDescriptor.class);
            return (Boolean) method.invoke(null, descriptor);
        } catch (ReflectiveOperationException unavailableOnOlderPlatform) {
            return PluginManagerCore.getLoadedPlugins().stream()
                .anyMatch(loaded -> loaded.getPluginId().equals(descriptor.getPluginId()));
        }
    }

}
