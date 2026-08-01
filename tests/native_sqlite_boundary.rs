use std::ffi::CString;

#[test]
fn automatic_editor_ffi_never_opens_or_bootstraps_the_controller_state_db() {
    let project = tempfile::TempDir::new().unwrap();
    let state_dir = project.path().join(".agent-doc");
    std::fs::create_dir_all(&state_dir).unwrap();
    let state_db = state_dir.join("state.db");
    let sentinel = b"not a sqlite database; editor clients must leave this untouched";
    std::fs::write(&state_db, sentinel).unwrap();

    let file = project.path().join("session.md");
    let content = "---\nagent: codex\nagent_doc_format: template\n---\n\n# session\n";
    std::fs::write(&file, content).unwrap();

    let root = CString::new(project.path().to_string_lossy().as_bytes()).unwrap();
    let file_c = CString::new(file.to_string_lossy().as_bytes()).unwrap();
    let content_c = CString::new(content).unwrap();
    let editor = CString::new("native-sqlite-boundary").unwrap();
    let kind = CString::new("test").unwrap();
    let version = CString::new("test").unwrap();
    let capabilities = CString::new("").unwrap();
    let patch_id = CString::new("native-sqlite-boundary-patch").unwrap();
    let file_json = file.to_string_lossy().into_owned();
    let surface = CString::new(
        serde_json::json!({
            "focused": file_json,
            "visible": [file_json],
            "open": [file_json],
            "columns": [],
            "force_reconcile": false
        })
        .to_string(),
    )
    .unwrap();

    let version_ptr = agent_doc::ffi::agent_doc_version();
    unsafe {
        agent_doc::ffi::agent_doc_free_string(version_ptr);
        agent_doc::ffi::agent_doc_lazily_current_observed_v1(
            file_c.as_ptr(),
            content_c.as_ptr(),
            editor.as_ptr(),
            kind.as_ptr(),
            version.as_ptr(),
            capabilities.as_ptr(),
            0,
        );
        assert_eq!(
            agent_doc::ffi::agent_doc_editor_patch_applied(file_c.as_ptr(), patch_id.as_ptr(), 1,),
            0,
            "without a controller, durable receipt ingress fails closed"
        );
        assert_eq!(
            agent_doc::ffi::agent_doc_editor_surface_enqueue_json(root.as_ptr(), surface.as_ptr(),),
            -1,
            "passive surface ingress never launches a controller"
        );
        let reconnect = agent_doc::ffi::agent_doc_deferred_write_reconnect_content(
            file_c.as_ptr(),
            content_c.as_ptr(),
        );
        assert!(
            reconnect.is_null(),
            "an unavailable controller is not replaced with a native SQLite fallback"
        );
    }

    assert_eq!(std::fs::read(&state_db).unwrap(), sentinel);
    assert!(!state_dir.join("state.db-wal").exists());
    assert!(!state_dir.join("state.db-shm").exists());
    assert!(!state_dir.join("controller.sock").exists());
}
