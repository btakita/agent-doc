package com.github.btakita.agentdoc;

import com.intellij.ide.plugins.DynamicPlugins;
import com.intellij.ide.plugins.IdeaPluginDescriptor;
import com.intellij.ide.plugins.IdeaPluginDescriptorImpl;
import com.intellij.ide.plugins.PluginInstaller;
import com.intellij.ide.plugins.PluginManagerCore;
import com.intellij.openapi.application.ApplicationManager;
import com.intellij.openapi.extensions.PluginDescriptor;
import com.intellij.openapi.extensions.PluginId;
import com.intellij.openapi.project.Project;
import com.intellij.openapi.project.ProjectManager;

import java.lang.reflect.Field;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.nio.file.Path;
import java.util.concurrent.atomic.AtomicReference;

/** Freshly loaded implementation of one JetBrains plugin package replacement. */
public final class JetBrainsPluginUpgradeAction {
    private static final String PLUGIN_ID = "com.github.btakita.agent-doc";
    private static final String LIFECYCLE_CLASS =
        "com.github.btakita.agentdoc.PluginLifecycleListener";

    private JetBrainsPluginUpgradeAction() {}

    public static String run(String archiveValue, String pluginsDirValue, String expectedVersion) {
        AtomicReference<String> result = new AtomicReference<>();
        AtomicReference<Throwable> failure = new AtomicReference<>();
        AtomicReference<IdeaPluginDescriptorImpl> replacement = new AtomicReference<>();
        ApplicationManager.getApplication().invokeAndWait(() -> {
            try {
                result.set(runOnEdt(archiveValue, pluginsDirValue, expectedVersion, replacement));
            } catch (Throwable caught) {
                failure.set(caught);
            }
        });
        if (failure.get() != null) {
            throw new IllegalStateException(failure.get().getMessage(), failure.get());
        }
        IdeaPluginDescriptorImpl loaded = replacement.get();
        if (loaded == null) {
            return result.get();
        }
        return result.get() + ":" + reattachOpenDocuments(loaded);
    }

    private static String runOnEdt(
        String archiveValue,
        String pluginsDirValue,
        String expectedVersion,
        AtomicReference<IdeaPluginDescriptorImpl> replacement
    ) {
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
            int cleanedProjects = cleanupOutgoingGeneration(current);
            DynamicPlugins.UnloadPluginOptions updateOptions =
                new DynamicPlugins.UnloadPluginOptions()
                    .withDisable(false)
                    .withUpdate(true);
            if (!DynamicPlugins.INSTANCE.unloadPlugin(current, updateOptions)) {
                throw new IllegalStateException(
                    "JetBrains refused to unload the current plugin generation after cleaning "
                        + cleanedProjects + " open project(s)"
                );
            }
        }

        IdeaPluginDescriptor residualDescriptor = PluginManagerCore.getPlugin(PluginId.getId(PLUGIN_ID));
        if (residualDescriptor instanceof IdeaPluginDescriptorImpl residual && isLoaded(residual)) {
            throw new IllegalStateException(
                "JetBrains retained the current plugin generation after update unload: "
                    + residual.getVersion()
            );
        }

        boolean loaded = PluginInstaller.installAndLoadDynamicPlugin(archive, current);
        IdeaPluginDescriptor actualDescriptor = PluginManagerCore.getPlugin(PluginId.getId(PLUGIN_ID));
        if (!(actualDescriptor instanceof IdeaPluginDescriptorImpl actual)
            || !loaded
            || actual == current
            || actual.getPluginClassLoader() == current.getPluginClassLoader()
            || !expectedVersion.equals(actual.getVersion())) {
            String actualVersion = actualDescriptor == null ? "missing" : actualDescriptor.getVersion();
            throw new IllegalStateException(
                "dynamic install returned " + loaded + "; expected a fresh " + expectedVersion
                    + " generation, loaded " + actualVersion
            );
        }
        replacement.set(actual);
        return "ok:" + expectedVersion;
    }

    /**
     * Dispose the current plugin generation before asking IntelliJ to detach its descriptor.
     *
     * Newer generations expose one public static hook. The companion fallback upgrades the
     * first generation that shipped dynamic unload but predates that hook; Kotlin's internal
     * method is public bytecode with a module suffix. Failure is closed because loading a
     * replacement beside live old-generation DocumentListeners creates a CRDT echo loop.
     */
    private static int cleanupOutgoingGeneration(IdeaPluginDescriptorImpl descriptor) {
        try {
            ClassLoader loader = descriptor.getPluginClassLoader();
            Class<?> lifecycle = Class.forName(LIFECYCLE_CLASS, true, loader);
            try {
                return (Integer) lifecycle
                    .getMethod("disposeOpenProjectsForDynamicUnload")
                    .invoke(null);
            } catch (NoSuchMethodException legacyGeneration) {
                Field companionField = lifecycle.getField("Companion");
                Object companion = companionField.get(null);
                Method cleanup = null;
                for (Method candidate : companion.getClass().getMethods()) {
                    if (candidate.getName().startsWith("disposeProjectResources$")
                        && candidate.getParameterCount() == 1
                        && candidate.getParameterTypes()[0] == Project.class) {
                        cleanup = candidate;
                        break;
                    }
                }
                if (cleanup == null) {
                    throw legacyGeneration;
                }
                Project[] projects = ProjectManager.getInstance().getOpenProjects();
                int cleaned = 0;
                for (Project project : projects) {
                    if (!project.isDisposed()) {
                        cleanup.invoke(companion, project);
                        cleaned++;
                    }
                }
                return cleaned;
            }
        } catch (ReflectiveOperationException failure) {
            Throwable cause = failure instanceof InvocationTargetException invocation
                && invocation.getCause() != null
                ? invocation.getCause()
                : failure;
            throw new IllegalStateException(
                "outgoing plugin generation could not release open projects: "
                    + cause.getMessage(),
                cause
            );
        }
    }

    /**
     * Ask the replacement generation to rebuild project services and reattach open documents,
     * then return its receipt.
     *
     * `#jbupgradereattach`: this runs only after the upgrade verdict is already decided.
     * {@code runOnEdt} returns {@code ok:} solely on converged bytes -- the package is installed
     * and a fresh descriptor of the expected version owns a new classloader. Open-document CRDT
     * re-registration is a different property, owned by whichever controller owns each document's
     * own project root, which this install does not own. So a shortfall, or even an unreachable
     * receipt, is reported here rather than raised: raising it failed the whole {@code make install}
     * after the replacement had already landed irreversibly, and the retry then correctly reported
     * the package byte-identical with no restart required.
     */
    private static String reattachOpenDocuments(IdeaPluginDescriptorImpl descriptor) {
        try {
            Class<?> lifecycle = Class.forName(
                LIFECYCLE_CLASS,
                true,
                descriptor.getPluginClassLoader()
            );
            return String.valueOf(
                lifecycle.getMethod("initializeOpenProjectsAfterDynamicLoad").invoke(null)
            );
        } catch (Throwable failure) {
            Throwable cause = failure instanceof InvocationTargetException invocation
                && invocation.getCause() != null
                ? invocation.getCause()
                : failure;
            return "documents=0/0:reattach_error=" + singleLine(cause);
        }
    }

    /** Keep a receipt to one line: the launcher reads the status file as a single status. */
    private static String singleLine(Throwable cause) {
        String message = cause.getMessage();
        String text = message == null || message.isBlank() ? cause.getClass().getName() : message;
        return text.replace('\n', ' ').replace('\r', ' ').trim();
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
