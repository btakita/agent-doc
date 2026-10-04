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
//!   build_id and the three expected-plugin keys.
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

/// Editor package generations this build expects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExpectedPlugins {
    pub jetbrains: Option<String>,
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
            vscode: expected("vscode"),
            zed: expected("zed"),
        }
    }
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
}

/// The shared library `agent-doc lib-path` resolves for `executable`.
pub fn sibling_library_path(executable: &Path) -> Option<PathBuf> {
    executable.parent().map(|dir| dir.join(LIBRARY_FILE_NAME))
}

/// Report for the CLI binary running at `executable`.
pub fn binary_build_info(executable: Option<&Path>) -> BuildInfo {
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
        "expected plugins: jetbrains {}, vscode {}, zed {}\n",
        or_unknown(&info.expected_plugins.jetbrains),
        or_unknown(&info.expected_plugins.vscode),
        or_unknown(&info.expected_plugins.zed),
    ));
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
        for key in ["jetbrains", "vscode", "zed"] {
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
            expected.vscode.as_deref(),
            agent_doc_reliable_sync_io::liveness::expected_editor_plugin_version("vscode")
        );
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
        info.expected_plugins = ExpectedPlugins {
            jetbrains: Some("0.2.1".into()),
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
            text.contains("expected plugins: jetbrains 0.2.1, vscode unknown, zed 0.1.1\n"),
            "{text}"
        );
    }
}
