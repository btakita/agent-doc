//! Machine-readable native bootstrap must stay usable with a newer cached release.

#[test]
fn lib_path_ignores_newer_release_notice_on_both_streams() {
    let tmp = tempfile::tempdir().unwrap();
    let binary = tmp.path().join("agent-doc");
    std::fs::copy(assert_cmd::cargo::cargo_bin!("agent-doc"), &binary).unwrap();
    let library = tmp.path().join(if cfg!(target_os = "macos") {
        "libagent_doc.dylib"
    } else if cfg!(target_os = "windows") {
        "agent_doc.dll"
    } else {
        "libagent_doc.so"
    });
    std::fs::write(&library, b"path discovery does not load the library").unwrap();
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
        .arg("lib-path")
        .env("HOME", tmp.path())
        .env_remove("AGENT_DOC_LOG")
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{}\n", library.display())
    );
    assert!(
        output.stderr.is_empty(),
        "{:?}",
        String::from_utf8_lossy(&output.stderr)
    );
}
