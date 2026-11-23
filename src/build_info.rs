//! # Module: build_info
//!
//! ## Spec
//! - One build-identity report shared by the CLI (`agent-doc version`) and the
//!   cdylib (`agent_doc_build_info_json`), so the editor "About Agent Doc"
//!   action compares like with like (`editoractionmenu`).
//! - `build_id` is the exact IPC build identity both processes put on the wire:
//!   `<CARGO_PKG_VERSION>+<workspace source digest>` (see `build.rs`). Two
//!   components with different build ids cannot complete the IPC handshake.
//! - `expected_plugins` carries the editor package generations this build was
//!   compiled against (`agent_doc_reliable_sync_io::liveness`), which is the
//!   same fence the controller applies to a registering editor replica.
//!   JetBrains ships two packages per release (GH #434): `jetbrains` is the
//!   classic ZIP for builds 242-261.*, `jetbrains_262` the one modular ZIP for
//!   builds 262 through 263.*. Reporting only the classic one made a 263 IDE
//!   running the release-paired modular plugin look like a mismatch.
//! - `installed_plugins` (binary only) names each IDE data directory's
//!   on-disk agent-doc JetBrains plugin, the package generation this build
//!   expects for that IDE's platform build, and whether the two match.
//! - `component` is `"binary"` for the CLI and `"native_library"` for the
//!   cdylib; the binary also reports its executable path and the sibling
//!   shared-library path that `agent-doc lib-path` would print.
//!
//! ## Agentic Contracts
//! - The JSON shape is versioned by `schema` (`agent-doc-build-info-v1`);
//!   fields are only ever added.
//! - A missing expectation (a build whose editor manifests were absent) is
//!   `null`, never a guessed value.
//!
//! ## Evals
//! - build_id_prefix: `BUILD_ID` starts with `VERSION` followed by `+`.
//! - json_shape: serialized report carries schema, component, version,
//!   build_id and the four expected-plugin keys.
//! - installed_plugin_expectation: a 263 IDE's modular plugin is compared with
//!   the modular generation, a 261 IDE's with the classic one.
//! - sibling_library: the library path is the platform cdylib name next to the
//!   executable.

use serde::Serialize;
use std::path::{Path, PathBuf};

/// Contract version for [`BuildInfo`] JSON.
pub const SCHEMA: &str = "agent-doc-build-info-v1";

/// The crate version this binary/library was built as.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The IPC build identity: `<version>+<workspace source digest>`.
pub const BUILD_ID: &str = concat!(env!("CARGO_PKG_VERSION"), "+", env!("AGENT_DOC_BUILD_ID"));

/// Platform file name of the agent-doc shared library.
#[cfg(target_os = "macos")]
pub const LIBRARY_FILE_NAME: &str = "libagent_doc.dylib";
#[cfg(target_os = "windows")]
pub const LIBRARY_FILE_NAME: &str = "agent_doc.dll";
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub const LIBRARY_FILE_NAME: &str = "libagent_doc.so";

/// JetBrains platform builds the modular package serves (`#jbdocdrift`).
pub const JETBRAINS_MODULAR_BUILDS: &str = "262-263.*";
/// JetBrains platform builds the classic package serves.
pub const JETBRAINS_CLASSIC_BUILDS: &str = "242-261.*";

/// Editor package generations this build expects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExpectedPlugins {
    /// The classic JetBrains package (`agent-doc-jetbrains-<v>.zip`, builds 242-261.*).
    pub jetbrains: Option<String>,
    /// GH #434: the modular JetBrains package (`agent-doc-jetbrains-262-<v>.zip`),
    /// one ZIP for builds 262 through 263.*.
    pub jetbrains_262: Option<String>,
    pub vscode: Option<String>,
    pub zed: Option<String>,
}

impl ExpectedPlugins {
    pub fn compiled() -> Self {
        let expected = |kind: &str| {
            agent_doc_reliable_sync_io::liveness::expected_editor_plugin_version(kind)
                .map(str::to_string)
        };
        Self {
            jetbrains: expected("jetbrains"),
            jetbrains_262:
                agent_doc_reliable_sync_io::liveness::expected_editor_plugin_version_for_platform_build(
                    "jetbrains",
                    Some(262),
                )
                .map(str::to_string),
            vscode: expected("vscode"),
            zed: expected("zed"),
        }
    }
}

/// GH #434: one IDE data directory's on-disk agent-doc JetBrains plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstalledEditorPlugin {
    pub editor: &'static str,
    /// The IDE data directory name, e.g. `IntelliJIdea2026.3`.
    pub ide: String,
    /// The plugin jar on disk.
    pub path: String,
    /// The installed package version.
    pub version: String,
    /// The JetBrains platform build the IDE directory names (`263`), if provable.
    pub platform_build: Option<u32>,
    /// The package generation this build expects for that platform build.
    pub expected: Option<String>,
    /// `version == expected`.
    pub current: bool,
}

/// Every installed agent-doc JetBrains plugin under `plugin_dirs`, each
/// compared with the package this build pairs with its IDE's platform build.
pub fn installed_jetbrains_plugins(plugin_dirs: &[PathBuf]) -> Vec<InstalledEditorPlugin> {
    let mut installed = Vec::new();
    for dir in plugin_dirs {
        let platform_build = agent_doc_fs::jetbrains_install::jetbrains_platform_build(dir);
        let ide_dir = if dir.file_name().is_some_and(|name| name == "plugins") {
            dir.parent().unwrap_or(dir)
        } else {
            dir.as_path()
        };
        let ide = ide_dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| ide_dir.display().to_string());
        let Some(artifact) = agent_doc_fs::jetbrains_install::installed_artifacts_in(dir)
            .into_iter()
            .max_by_key(|artifact| artifact.modified)
        else {
            continue;
        };
        let expected =
            agent_doc_reliable_sync_io::liveness::expected_editor_plugin_version_for_platform_build(
                "jetbrains",
                platform_build,
            )
            .map(str::to_string);
        installed.push(InstalledEditorPlugin {
            editor: "jetbrains",
            ide,
            path: artifact.path.display().to_string(),
            current: expected.as_deref() == Some(artifact.version.as_str()),
            version: artifact.version,
            platform_build,
            expected,
        });
    }
    installed
}

/// Build identity of one agent-doc component.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BuildInfo {
    pub schema: &'static str,
    pub component: &'static str,
    pub version: String,
    pub build_id: String,
    /// The running executable (binary component only).
    pub executable: Option<String>,
    /// The shared library paired with the executable: the sibling path
    /// `agent-doc lib-path` resolves, when it exists (binary component only).
    pub library: Option<String>,
    pub expected_plugins: ExpectedPlugins,
    /// GH #434: on-disk editor plugins (binary component only; empty for the
    /// native library, which reports no paths).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub installed_plugins: Vec<InstalledEditorPlugin>,
}

/// The shared library `agent-doc lib-path` resolves for `executable`.
pub fn sibling_library_path(executable: &Path) -> Option<PathBuf> {
    executable.parent().map(|dir| dir.join(LIBRARY_FILE_NAME))
}

/// Report for the CLI binary running at `executable`.
pub fn binary_build_info(executable: Option<&Path>) -> BuildInfo {
    binary_build_info_with_plugin_dirs(
        executable,
        &agent_doc_fs::jetbrains_install::jetbrains_plugin_dirs(),
    )
}

/// [`binary_build_info`] over explicit JetBrains plugin directories.
pub fn binary_build_info_with_plugin_dirs(
    executable: Option<&Path>,
    jetbrains_plugin_dirs: &[PathBuf],
) -> BuildInfo {
    let library = executable
        .and_then(sibling_library_path)
        .filter(|path| path.exists())
        .map(|path| path.display().to_string());
    BuildInfo {
        schema: SCHEMA,
        component: "binary",
        version: VERSION.to_string(),
        build_id: BUILD_ID.to_string(),
        executable: executable.map(|path| path.display().to_string()),
        library,
        expected_plugins: ExpectedPlugins::compiled(),
        installed_plugins: installed_jetbrains_plugins(jetbrains_plugin_dirs),
    }
}

/// Report for the loaded native library (the cdylib does not know its own
/// path; the editor that loaded it does).
pub fn native_library_build_info() -> BuildInfo {
    BuildInfo {
        schema: SCHEMA,
        component: "native_library",
        version: VERSION.to_string(),
        build_id: BUILD_ID.to_string(),
        executable: None,
        library: None,
        expected_plugins: ExpectedPlugins::compiled(),
        installed_plugins: Vec::new(),
    }
}

/// Human-readable form of [`BuildInfo`] for `agent-doc version`.
pub fn render_text(info: &BuildInfo) -> String {
    let or_unknown = |value: &Option<String>| value.clone().unwrap_or_else(|| "unknown".into());
    let mut out = format!("agent-doc {}\n", info.version);
    out.push_str(&format!("build id: {}\n", info.build_id));
    if info.component == "binary" {
        out.push_str(&format!("executable: {}\n", or_unknown(&info.executable)));
        out.push_str(&format!(
            "native library: {}\n",
            info.library.clone().unwrap_or_else(|| "not found".into())
        ));
    }
    out.push_str(&format!(
        "expected plugins: jetbrains {} (builds {JETBRAINS_CLASSIC_BUILDS}), jetbrains-262 {} (builds {JETBRAINS_MODULAR_BUILDS}), vscode {}, zed {}\n",
        or_unknown(&info.expected_plugins.jetbrains),
        or_unknown(&info.expected_plugins.jetbrains_262),
        or_unknown(&info.expected_plugins.vscode),
        or_unknown(&info.expected_plugins.zed),
    ));
    for plugin in &info.installed_plugins {
        let build = plugin
            .platform_build
            .map(|build| format!(" (build {build})"))
            .unwrap_or_default();
        let verdict = match (&plugin.expected, plugin.current) {
            (_, true) => "current".to_string(),
            (Some(expected), false) => format!("expects {expected}"),
            (None, false) => "no expected generation".to_string(),
        };
        out.push_str(&format!(
            "installed {} plugin: {}{build} {} ({verdict})\n",
            plugin.editor, plugin.ide, plugin.version,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_id_is_version_plus_source_digest() {
        let digest = BUILD_ID
            .strip_prefix(&format!("{VERSION}+"))
            .expect("build id starts with the version");
        assert!(!digest.is_empty());
        assert!(!digest.contains('+'));
    }

    #[test]
    fn build_info_json_carries_the_versioned_contract() {
        let info = binary_build_info(Some(Path::new("/nonexistent/bin/agent-doc")));
        let value = serde_json::to_value(&info).unwrap();
        assert_eq!(value["schema"], SCHEMA);
        assert_eq!(value["component"], "binary");
        assert_eq!(value["version"], VERSION);
        assert_eq!(value["build_id"], BUILD_ID);
        assert_eq!(value["executable"], "/nonexistent/bin/agent-doc");
        assert!(value["library"].is_null(), "a missing library is null");
        for key in ["jetbrains", "jetbrains_262", "vscode", "zed"] {
            assert!(
                value["expected_plugins"].get(key).is_some(),
                "expected_plugins.{key} must be present even when null"
            );
        }
    }

    #[test]
    fn expected_plugins_match_the_registration_fence() {
        let expected = ExpectedPlugins::compiled();
        assert_eq!(
            expected.jetbrains.as_deref(),
            agent_doc_reliable_sync_io::liveness::expected_editor_plugin_version("jetbrains")
        );
        assert_eq!(
            expected.jetbrains_262.as_deref(),
            agent_doc_reliable_sync_io::liveness::expected_editor_plugin_version_for_platform_build(
                "jetbrains",
                Some(263),
            ),
            "GH #434: one modular ZIP serves 262 through 263.*"
        );
        assert_eq!(
            expected.vscode.as_deref(),
            agent_doc_reliable_sync_io::liveness::expected_editor_plugin_version("vscode")
        );
    }

    fn install_jar(plugins: &Path, package_dir: &str, jar: &str) {
        let lib = plugins.join(package_dir).join("lib");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join(jar), b"jar").unwrap();
    }

    /// GH #434: a 263 IDE running the release-paired modular plugin was
    /// reported against the classic generation (`expects 0.2.1024` beside an
    /// on-disk `0.2.1029`). Each IDE is compared with its own package.
    #[test]
    fn installed_plugins_compare_each_ide_with_its_own_package() {
        let expected = ExpectedPlugins::compiled();
        let (Some(classic), Some(modular)) = (expected.jetbrains, expected.jetbrains_262) else {
            return;
        };
        let tmp = tempfile::tempdir().unwrap();
        let idea263 = tmp.path().join("IntelliJIdea2026.3");
        let rider262 = tmp.path().join("Rider2026.2");
        let idea261 = tmp.path().join("IntelliJIdea2026.1");
        let stale = tmp.path().join("PyCharm2026.3");
        install_jar(
            &idea263,
            "agent-doc-jetbrains",
            &format!("agent.doc-{modular}.jar"),
        );
        install_jar(
            &rider262,
            "agent-doc-jetbrains-262",
            &format!("agent.doc-{modular}.jar"),
        );
        install_jar(
            &idea261,
            "agent-doc-jetbrains",
            &format!("agent-doc-jetbrains-{classic}.jar"),
        );
        install_jar(&stale, "agent-doc-jetbrains", "agent.doc-0.2.1.jar");
        let dirs = [
            idea261.clone(),
            idea263.clone(),
            rider262.clone(),
            stale.clone(),
        ];
        let installed = installed_jetbrains_plugins(&dirs);
        let by_ide = |ide: &str| installed.iter().find(|plugin| plugin.ide == ide).unwrap();

        let idea = by_ide("IntelliJIdea2026.3");
        assert_eq!(idea.platform_build, Some(263));
        assert_eq!(idea.expected.as_deref(), Some(modular.as_str()));
        assert!(idea.current, "{idea:?}");
        assert!(by_ide("Rider2026.2").current);
        let old = by_ide("IntelliJIdea2026.1");
        assert_eq!(old.expected.as_deref(), Some(classic.as_str()));
        assert!(old.current, "{old:?}");
        let pycharm = by_ide("PyCharm2026.3");
        assert!(!pycharm.current);
        assert_eq!(pycharm.expected.as_deref(), Some(modular.as_str()));

        let mut info = binary_build_info_with_plugin_dirs(Some(Path::new("/x/agent-doc")), &dirs);
        info.expected_plugins.vscode = None;
        let text = render_text(&info);
        assert!(
            text.contains(&format!(
                "installed jetbrains plugin: IntelliJIdea2026.3 (build 263) {modular} (current)\n"
            )),
            "{text}"
        );
        assert!(
            text.contains(&format!(
                "installed jetbrains plugin: PyCharm2026.3 (build 263) 0.2.1 (expects {modular})\n"
            )),
            "{text}"
        );
        let value = serde_json::to_value(&info).unwrap();
        assert_eq!(value["installed_plugins"].as_array().unwrap().len(), 4);
        assert_eq!(value["expected_plugins"]["jetbrains_262"], modular.as_str());
    }

    #[test]
    fn no_installed_plugin_omits_the_field() {
        let info = binary_build_info_with_plugin_dirs(None, &[]);
        let value = serde_json::to_value(&info).unwrap();
        assert!(value.get("installed_plugins").is_none(), "{value}");
    }

    #[test]
    fn native_library_report_has_no_paths() {
        let info = native_library_build_info();
        assert_eq!(info.component, "native_library");
        assert_eq!(info.build_id, BUILD_ID);
        assert!(info.executable.is_none() && info.library.is_none());
    }

    #[test]
    fn sibling_library_sits_next_to_the_executable() {
        let path = sibling_library_path(Path::new("/opt/agent-doc/bin/agent-doc")).unwrap();
        assert_eq!(
            path,
            Path::new("/opt/agent-doc/bin").join(LIBRARY_FILE_NAME)
        );
    }

    #[test]
    fn existing_sibling_library_is_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let exe = tmp.path().join("agent-doc");
        std::fs::write(tmp.path().join(LIBRARY_FILE_NAME), b"lib").unwrap();
        let info = binary_build_info(Some(&exe));
        assert_eq!(
            info.library.as_deref(),
            Some(
                tmp.path()
                    .join(LIBRARY_FILE_NAME)
                    .display()
                    .to_string()
                    .as_str()
            )
        );
    }

    #[test]
    fn text_report_names_every_identity_field() {
        let mut info = binary_build_info(Some(Path::new("/x/agent-doc")));
        info.installed_plugins.clear();
        info.expected_plugins = ExpectedPlugins {
            jetbrains: Some("0.2.1".into()),
            jetbrains_262: Some("0.2.2".into()),
            vscode: None,
            zed: Some("0.1.1".into()),
        };
        let text = render_text(&info);
        assert!(
            text.starts_with(&format!("agent-doc {VERSION}\n")),
            "{text}"
        );
        assert!(text.contains(&format!("build id: {BUILD_ID}\n")), "{text}");
        assert!(text.contains("executable: /x/agent-doc\n"), "{text}");
        assert!(text.contains("native library: not found\n"), "{text}");
        assert!(
            text.contains(
                "expected plugins: jetbrains 0.2.1 (builds 242-261.*), jetbrains-262 0.2.2 (builds 262-263.*), vscode unknown, zed 0.1.1\n"
            ),
            "{text}"
        );
    }
}
