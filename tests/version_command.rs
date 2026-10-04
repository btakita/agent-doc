//! `editoractionmenu`: `agent-doc version --json` is the CLI half of the editor
//! "About Agent Doc" action. It must report the same build identity the native
//! library reports, stay machine-readable on stdout, and never carry a startup
//! upgrade notice.

use std::ffi::CStr;

fn run_version(args: &[&str]) -> (std::process::Output, std::path::PathBuf, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let binary = tmp.path().join("agent-doc");
    std::fs::copy(assert_cmd::cargo::cargo_bin!("agent-doc"), &binary).unwrap();
    let cache_dir = tmp.path().join(".cache/agent-doc");
    std::fs::create_dir_all(&cache_dir).unwrap();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    std::fs::write(
        cache_dir.join("version-cache.json"),
        serde_json::json!({"timestamp": timestamp, "version": "999.0.0"}).to_string(),
    )
    .unwrap();
    let output = std::process::Command::new(&binary)
        .arg("version")
        .args(args)
        .env("HOME", tmp.path())
        .env_remove("AGENT_DOC_LOG")
        .current_dir(tmp.path())
        .output()
        .unwrap();
    (output, binary, tmp)
}

#[test]
fn version_json_reports_the_binary_build_identity_without_notices() {
    let (output, binary, tmp) = run_version(&["--json"]);
    assert!(output.status.success(), "{output:?}");
    assert!(
        output.stderr.is_empty(),
        "{:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema"], agent_doc::build_info::SCHEMA);
    assert_eq!(value["component"], "binary");
    assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(value["build_id"], agent_doc::build_info::BUILD_ID);
    let reported = std::path::PathBuf::from(value["executable"].as_str().unwrap());
    assert_eq!(
        reported.canonicalize().unwrap(),
        binary.canonicalize().unwrap()
    );
    assert!(
        value["library"].is_null(),
        "no sibling library was installed in {}",
        tmp.path().display()
    );
    let gradle = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/editors/jetbrains/gradle.properties"
    ))
    .unwrap();
    let jetbrains = gradle
        .lines()
        .find_map(|line| line.trim().strip_prefix("pluginVersion"))
        .and_then(|rest| rest.trim_start().strip_prefix('='))
        .map(str::trim)
        .unwrap();
    assert_eq!(value["expected_plugins"]["jetbrains"], jetbrains);
}

#[test]
fn version_json_names_the_sibling_native_library() {
    let tmp = tempfile::tempdir().unwrap();
    let binary = tmp.path().join("agent-doc");
    std::fs::copy(assert_cmd::cargo::cargo_bin!("agent-doc"), &binary).unwrap();
    let library = tmp.path().join(agent_doc::build_info::LIBRARY_FILE_NAME);
    std::fs::write(&library, b"version discovery does not load the library").unwrap();
    let output = std::process::Command::new(&binary)
        .args(["version", "--json"])
        .env("HOME", tmp.path())
        .env_remove("AGENT_DOC_LOG")
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        std::path::PathBuf::from(value["library"].as_str().unwrap())
            .canonicalize()
            .unwrap(),
        library.canonicalize().unwrap()
    );
}

#[test]
fn version_text_leads_with_the_clap_version_line() {
    let (output, _binary, _tmp) = run_version(&[]);
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    // Same first line as `agent-doc --version`, so older parsers keep working.
    assert!(
        text.starts_with(&format!("agent-doc {}\n", env!("CARGO_PKG_VERSION"))),
        "{text}"
    );
    assert!(
        text.contains(&format!("build id: {}\n", agent_doc::build_info::BUILD_ID)),
        "{text}"
    );
}

#[test]
fn native_library_build_info_matches_the_binary_build_id() {
    let ptr = agent_doc::ffi::agent_doc_build_info_json();
    assert!(!ptr.is_null());
    let json = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_string();
    unsafe { agent_doc::ffi::agent_doc_free_string(ptr) };
    let native: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(native["schema"], agent_doc::build_info::SCHEMA);
    assert_eq!(native["component"], "native_library");

    let (output, _binary, _tmp) = run_version(&["--json"]);
    let cli: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        native["build_id"], cli["build_id"],
        "the cdylib and the binary built from one tree must share an IPC build id"
    );
    assert_eq!(native["expected_plugins"], cli["expected_plugins"]);
}
