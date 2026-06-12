    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup() -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "# Test\n").unwrap();
        (dir, doc)
    }

    /// Helper: write a snapshot file directly (without changing CWD).
    fn write_snapshot_directly(dir: &Path, doc: &Path, content: &str) {
        let snap = snapshot_path_in(dir, doc);
        fs::create_dir_all(snap.parent().unwrap()).unwrap();
        fs::write(&snap, content).unwrap();
    }

    /// Helper: read a snapshot file directly (without changing CWD).
    fn read_snapshot_directly(dir: &Path, doc: &Path) -> Option<String> {
        let snap = snapshot_path_in(dir, doc);
        if snap.exists() {
            Some(fs::read_to_string(&snap).unwrap())
        } else {
            None
        }
    }

    /// Compute snapshot path within a specific directory.
    /// If path_for returns absolute (project root found), use it directly.
    /// Otherwise, join relative path with dir.
    fn snapshot_path_in(dir: &Path, doc: &Path) -> PathBuf {
        let p = path_for(doc).unwrap();
        if p.is_absolute() { p } else { dir.join(&p) }
    }

    #[test]
    fn path_for_consistent_hash() {
        let (_dir, doc) = setup();
        let p1 = path_for(&doc).unwrap();
        let p2 = path_for(&doc).unwrap();
        assert_eq!(p1, p2);
    }

    #[test]
    fn path_for_different_files_different_hashes() {
        let dir = TempDir::new().unwrap();
        let doc_a = dir.path().join("a.md");
        let doc_b = dir.path().join("b.md");
        fs::write(&doc_a, "a").unwrap();
        fs::write(&doc_b, "b").unwrap();
        let pa = path_for(&doc_a).unwrap();
        let pb = path_for(&doc_b).unwrap();
        assert_ne!(pa, pb);
    }

    #[test]
    fn path_for_has_correct_structure() {
        let (_dir, doc) = setup();
        let p = path_for(&doc).unwrap();
        assert!(p.to_string_lossy().contains(".agent-doc/snapshots/"));
        assert!(p.to_string_lossy().ends_with(".md"));
        // Hash is 64 hex chars
        let filename = p.file_stem().unwrap().to_string_lossy();
        assert_eq!(filename.len(), 64);
        assert!(filename.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn load_returns_none_when_no_snapshot() {
        let (_dir, doc) = setup();
        let result = load(&doc).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn snapshot_write_and_read_directly() {
        let (dir, doc) = setup();
        let content = "# Snapshot content\n\nWith body.\n";
        write_snapshot_directly(dir.path(), &doc, content);
        let loaded = read_snapshot_directly(dir.path(), &doc);
        assert_eq!(loaded.as_deref(), Some(content));
    }

    #[test]
    fn snapshot_overwrite() {
        let (dir, doc) = setup();
        write_snapshot_directly(dir.path(), &doc, "first");
        write_snapshot_directly(dir.path(), &doc, "second");
        let loaded = read_snapshot_directly(dir.path(), &doc);
        assert_eq!(loaded.as_deref(), Some("second"));
    }

    #[test]
    fn snapshot_delete_by_removing_file() {
        let (dir, doc) = setup();
        write_snapshot_directly(dir.path(), &doc, "content");
        assert!(read_snapshot_directly(dir.path(), &doc).is_some());

        let snap = snapshot_path_in(dir.path(), &doc);
        fs::remove_file(&snap).unwrap();
        assert!(read_snapshot_directly(dir.path(), &doc).is_none());
    }

    #[test]
    fn delete_no_error_when_missing() {
        let (_dir, doc) = setup();
        delete(&doc).unwrap();
    }

    // -----------------------------------------------------------------------
    // Race condition tests
    // -----------------------------------------------------------------------

    /// Test that flock-based locking works: acquire, hold, release on drop.
    /// Uses raw fs2 flock to avoid SnapshotLock's dependency on path_for/CWD.
    #[test]
    fn flock_acquire_and_release_on_drop() {
        use fs2::FileExt;
        use std::fs::OpenOptions;

        let dir = TempDir::new().unwrap();
        let lock_path = dir.path().join("test.lock");

        // First acquire succeeds
        {
            let file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(false)
                .open(&lock_path)
                .unwrap();
            file.lock_exclusive().unwrap();
            // Lock held here
            file.unlock().unwrap();
        }

        // After drop/unlock, second acquire succeeds
        let file2 = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .unwrap();
        file2.lock_exclusive().unwrap();
        file2.unlock().unwrap();
    }

    /// Test that concurrent flock acquisitions serialize properly
    /// (no data loss when multiple threads write through locks).
    #[test]
    fn flock_serializes_concurrent_access() {
        use fs2::FileExt;
        use std::sync::{Arc, Barrier};

        let dir = TempDir::new().unwrap();
        let lock_path = dir.path().join("test.lock");
        let data_path = dir.path().join("data.txt");
        fs::write(&data_path, "0").unwrap();

        let n = 10usize;
        let barrier = Arc::new(Barrier::new(n));
        let mut handles = Vec::new();

        for _ in 0..n {
            let lp = lock_path.clone();
            let dp = data_path.clone();
            let bar = Arc::clone(&barrier);
            handles.push(std::thread::spawn(move || {
                bar.wait();
                let file = std::fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(false)
                    .open(&lp)
                    .unwrap();
                file.lock_exclusive().unwrap();
                // Read-modify-write under lock
                let val: usize = fs::read_to_string(&dp).unwrap().trim().parse().unwrap();
                fs::write(&dp, (val + 1).to_string()).unwrap();
                file.unlock().unwrap();
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        let final_val: usize = fs::read_to_string(&data_path)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_eq!(final_val, n, "all {} increments should be serialized", n);
    }

    #[test]
    fn atomic_write_via_tempfile_produces_correct_content() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("output.md");

        // Atomic write: tempfile + persist
        let parent = dir.path();
        let mut tmp = tempfile::NamedTempFile::new_in(parent).unwrap();
        std::io::Write::write_all(&mut tmp, b"atomic content").unwrap();
        tmp.persist(&target).unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "atomic content");
    }

    #[test]
    fn atomic_write_overwrites_existing() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("output.md");
        fs::write(&target, "old").unwrap();

        let mut tmp = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
        std::io::Write::write_all(&mut tmp, b"new").unwrap();
        tmp.persist(&target).unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
    }

    #[test]
    fn crdt_path_has_correct_extension() {
        let (_dir, doc) = setup();
        let p = crdt_path_for(&doc).unwrap();
        assert!(p.to_string_lossy().contains(".agent-doc/crdt/"));
        assert!(p.to_string_lossy().ends_with(".yrs"));
    }

    #[test]
    fn overlay_crdt_path_has_correct_extension() {
        let (_dir, doc) = setup();
        let p = overlay_crdt_path_for(&doc).unwrap();
        assert!(p.to_string_lossy().contains(".agent-doc/crdt/"));
        assert!(p.to_string_lossy().ends_with(".overlay.yrs"));
    }

    #[test]
    fn crdt_save_and_load_roundtrip() {
        let (_dir, doc) = setup();
        let state = vec![1u8, 2, 3, 4, 5];
        save_crdt(&doc, &state).unwrap();
        let loaded = load_crdt(&doc).unwrap();
        assert_eq!(loaded, Some(state));
    }

    #[test]
    fn overlay_crdt_save_and_load_roundtrip() {
        let (_dir, doc) = setup();
        let state = vec![5u8, 4, 3, 2, 1];
        save_overlay_crdt(&doc, &state).unwrap();
        let loaded = load_overlay_crdt(&doc).unwrap();
        assert_eq!(loaded, Some(state));
    }

    #[test]
    fn document_crdt_save_persists_legacy_and_overlay_state() {
        let (_dir, doc) = setup();
        let markdown = concat!(
            "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "## Queue\n\n<!-- agent:queue -->\n",
            "- do [#first]\n",
            "<!-- /agent:queue -->\n"
        );
        let legacy = crate::crdt::CrdtDoc::from_text(markdown).encode_state();

        save_document_crdt(&doc, &legacy, markdown).unwrap();

        let loaded_legacy = load_crdt(&doc).unwrap().unwrap();
        assert_eq!(
            crate::crdt::CrdtDoc::decode_state(&loaded_legacy)
                .unwrap()
                .to_text(),
            markdown
        );
        let loaded_overlay = load_overlay_crdt(&doc).unwrap().unwrap();
        let overlay =
            agent_doc_markdown_ast::crdt::OverlayCrdtDoc::decode_state(&loaded_overlay).unwrap();
        assert_eq!(overlay.to_markdown().unwrap(), markdown);
        let components = overlay.to_components().unwrap();
        assert!(components.iter().any(|component| component.name == "queue"));
    }

    #[test]
    fn crdt_merge_base_state_prefers_matching_overlay_projection() {
        let (_dir, doc) = setup();
        let markdown = concat!(
            "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "## Queue\n\n<!-- agent:queue -->\n",
            "- do [#overlay-base]\n",
            "<!-- /agent:queue -->\n"
        );
        let stale_legacy = crate::crdt::CrdtDoc::from_text("stale legacy text").encode_state();
        save_document_crdt(&doc, &stale_legacy, markdown).unwrap();

        let base = crdt_merge_base_state(&doc, markdown).unwrap();

        assert_eq!(base.source, CrdtMergeBaseSource::Overlay);
        assert_eq!(
            crate::crdt::CrdtDoc::decode_state(&base.state)
                .unwrap()
                .to_text(),
            markdown
        );
    }

    #[test]
    fn crdt_merge_base_state_falls_back_when_overlay_projection_is_stale() {
        let (_dir, doc) = setup();
        let baseline = concat!(
            "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "## Queue\n\n<!-- agent:queue -->\n",
            "- do [#baseline]\n",
            "<!-- /agent:queue -->\n"
        );
        let stale_overlay = concat!(
            "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "## Queue\n\n<!-- agent:queue -->\n",
            "- do [#stale]\n",
            "<!-- /agent:queue -->\n"
        );
        let overlay_state =
            agent_doc_markdown_ast::crdt::OverlayCrdtDoc::from_markdown(stale_overlay)
                .encode_state();
        save_overlay_crdt(&doc, &overlay_state).unwrap();

        let base = crdt_merge_base_state(&doc, baseline).unwrap();

        assert_eq!(
            base.source,
            CrdtMergeBaseSource::FallbackOverlayProjectionMismatch
        );
        assert_eq!(
            crate::crdt::CrdtDoc::decode_state(&base.state)
                .unwrap()
                .to_text(),
            baseline
        );
    }

    /// #ipc-drift-order-stable-merge: the overlay-as-merge-base path (suspect
    /// 5fd64b26) must stay order-stable for the append case. With a prior
    /// committed `### Re:` response in the baseline, a foreign tail append
    /// landing during generation must not reverse the new response's lines or
    /// hoist it above the prior committed response. This drives the real
    /// `crdt_merge_base_state` overlay path end-to-end (not a hand-built
    /// `from_text` base), so a non-byte-stable overlay projection of an
    /// exchange-with-response document is caught here.
    #[test]
    fn overlay_merge_base_is_order_stable_for_exchange_append() {
        let (_dir, doc) = setup();
        let header = "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n";
        let exchange_committed = "\
❯ first question

### Re: first question — opus-4-8

First answer here. Already committed to HEAD.

❯ second question
";
        let queue_open = "<!-- /agent:exchange -->\n\n## Queue\n\n<!-- agent:queue -->\n";
        let queue_close = "<!-- /agent:queue -->\n";

        // Baseline: prior response committed, new prompt typed, boundary at tail.
        let base_markdown = format!(
            "{header}{exchange_committed}<!-- agent:boundary:base-id -->\n{queue_open}{queue_close}"
        );

        // Persist the overlay sidecar from the baseline (what a prior cycle saves).
        let stale_legacy = crate::crdt::CrdtDoc::from_text("stale legacy text").encode_state();
        save_document_crdt(&doc, &stale_legacy, &base_markdown).unwrap();

        // The overlay merge base must project to exactly the baseline text — the
        // order-stability guarantee — whether it uses the overlay or falls back.
        let base = crdt_merge_base_state(&doc, &base_markdown).unwrap();
        assert_eq!(
            crate::crdt::CrdtDoc::decode_state(&base.state)
                .unwrap()
                .to_text(),
            base_markdown,
            "overlay merge base is not byte-stable against the baseline (source={})",
            base.source.as_str()
        );

        // Ours: agent replaces the boundary with a new multi-line response.
        let agent_response = "\
### Re: second question — opus-4-8

Second answer line one.
Second answer line two.
Second answer line three.

";
        let ours = format!(
            "{header}{exchange_committed}{agent_response}<!-- agent:boundary:new-id -->\n{queue_open}{queue_close}"
        );
        // Theirs: a foreign supervisor appended a queue item at the tail mid-cycle.
        let theirs = format!(
            "{header}{exchange_committed}<!-- agent:boundary:base-id -->\n{queue_open}- do [#foreign-task]\n{queue_close}"
        );

        let merged = crate::crdt::merge(Some(&base.state), &ours, &theirs).unwrap();

        assert!(
            merged.contains("### Re: second question — opus-4-8"),
            "new response heading dropped under overlay base:\n{merged}"
        );
        let l1 = merged
            .find("Second answer line one.")
            .expect("line one missing");
        let l2 = merged
            .find("Second answer line two.")
            .expect("line two missing");
        let l3 = merged
            .find("Second answer line three.")
            .expect("line three missing");
        assert!(
            l1 < l2 && l2 < l3,
            "response lines reversed under overlay base (l1={l1} l2={l2} l3={l3}):\n{merged}"
        );
        let prior = merged
            .find("### Re: first question")
            .expect("prior response missing");
        let current = merged.find("### Re: second question").unwrap();
        assert!(
            prior < current,
            "new response hoisted above prior HEAD response under overlay base (prior={prior} current={current}):\n{merged}"
        );
        assert!(
            merged.contains("do [#foreign-task]"),
            "foreign queue append lost under overlay base:\n{merged}"
        );
    }

    #[test]
    fn crdt_load_returns_none_when_missing() {
        let (_dir, doc) = setup();
        let loaded = load_crdt(&doc).unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn crdt_delete_removes_file() {
        let (_dir, doc) = setup();
        save_crdt(&doc, &[1, 2, 3]).unwrap();
        save_overlay_crdt(&doc, &[4, 5, 6]).unwrap();
        assert!(load_crdt(&doc).unwrap().is_some());
        assert!(load_overlay_crdt(&doc).unwrap().is_some());
        delete_crdt(&doc).unwrap();
        assert!(load_crdt(&doc).unwrap().is_none());
        assert!(load_overlay_crdt(&doc).unwrap().is_none());
    }

    #[test]
    fn crdt_delete_no_error_when_missing() {
        let (_dir, doc) = setup();
        delete_crdt(&doc).unwrap();
    }

    #[test]
    fn concurrent_atomic_writes_no_partial_content() {
        use std::sync::{Arc, Barrier};

        let dir = TempDir::new().unwrap();
        let target = dir.path().join("concurrent.md");
        fs::write(&target, "initial").unwrap();

        let n = 20;
        let barrier = Arc::new(Barrier::new(n));
        let mut handles = Vec::new();

        for i in 0..n {
            let path = target.clone();
            let parent = dir.path().to_path_buf();
            let bar = Arc::clone(&barrier);
            let content = format!("writer-{}-content", i);
            handles.push(std::thread::spawn(move || {
                bar.wait();
                let mut tmp = tempfile::NamedTempFile::new_in(&parent).unwrap();
                std::io::Write::write_all(&mut tmp, content.as_bytes()).unwrap();
                tmp.persist(&path).unwrap();
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        // Final content must be exactly one valid write (no corruption/partial)
        let final_content = fs::read_to_string(&target).unwrap();
        assert!(
            final_content.starts_with("writer-") && final_content.ends_with("-content"),
            "unexpected content: {}",
            final_content
        );
    }

    #[test]
    fn resolve_prefers_snapshot_over_git() {
        // Verify that resolve() uses the snapshot file when it exists,
        // regardless of git commit mtime. This prevents the bug where
        // step 0b commit makes git newer than snapshot, causing resolve()
        // to return git content (= current file) instead of the snapshot.
        let (dir, doc) = setup();
        let snapshot_content = "snapshot baseline content";
        write_snapshot_directly(dir.path(), &doc, snapshot_content);

        // Even though the doc file on disk has different content,
        // resolve should return the snapshot file content.
        let resolved = resolve(&doc).unwrap();
        assert_eq!(
            resolved.as_deref(),
            Some(snapshot_content),
            "resolve() should always prefer snapshot file when it exists"
        );
    }

    // -----------------------------------------------------------------------
    // ensure_session_uuid tests
    // -----------------------------------------------------------------------

    fn setup_with_frontmatter(content: &str) -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, content).unwrap();
        (dir, doc)
    }

    #[test]
    fn ensure_session_uuid_assigns_to_template_without_session() {
        let (_dir, doc) = setup_with_frontmatter(
            "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n",
        );
        let assigned = ensure_session_uuid(&doc).unwrap();
        assert!(
            assigned,
            "should assign UUID to template file without session"
        );

        let content = fs::read_to_string(&doc).unwrap();
        assert!(
            content.contains("agent_doc_session:"),
            "file should have session UUID"
        );
    }

    #[test]
    fn ensure_session_uuid_noop_when_session_exists() {
        let (_dir, doc) = setup_with_frontmatter(
            "---\nagent_doc_session: existing-uuid\nagent_doc_format: template\n---\n\nBody\n",
        );
        let assigned = ensure_session_uuid(&doc).unwrap();
        assert!(!assigned, "should not reassign when session already exists");

        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("existing-uuid"), "original UUID preserved");
    }

    #[test]
    fn ensure_session_uuid_noop_when_no_format() {
        let (_dir, doc) =
            setup_with_frontmatter("---\ntitle: plain doc\n---\n\nNo agent_doc_format\n");
        let assigned = ensure_session_uuid(&doc).unwrap();
        assert!(!assigned, "should not assign UUID to non-agent-doc files");
    }

    // -----------------------------------------------------------------------
    // ensure_snapshot tests
    // -----------------------------------------------------------------------

    #[test]
    fn ensure_snapshot_creates_when_missing() {
        let (dir, doc) = setup_with_frontmatter(
            "---\nagent_doc_session: test-uuid\nagent_doc_format: template\n---\n\n<!-- agent:exchange patch=append -->\nuser text\n<!-- /agent:exchange -->\n",
        );
        // Create .agent-doc dir so snapshot path resolves
        fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();

        let created = ensure_snapshot(&doc).unwrap();
        assert!(created, "should create snapshot when none exists");

        let snap = read_snapshot_directly(dir.path(), &doc);
        assert!(snap.is_some(), "snapshot file should exist");
        // Exchange content should be stripped
        let snap_content = snap.unwrap();
        assert!(
            !snap_content.contains("user text"),
            "snapshot should have stripped exchange content"
        );
    }

    #[test]
    fn ensure_snapshot_noop_when_exists() {
        let (dir, doc) = setup();
        write_snapshot_directly(dir.path(), &doc, "existing snapshot");

        let created = ensure_snapshot(&doc).unwrap();
        assert!(!created, "should not recreate when snapshot exists");

        let snap = read_snapshot_directly(dir.path(), &doc).unwrap();
        assert_eq!(snap, "existing snapshot", "existing snapshot preserved");
    }

    // -----------------------------------------------------------------------
    // ensure_initialized composite tests
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // try_migrate_renamed tests
    // -----------------------------------------------------------------------

    fn setup_project_with_session(session_uuid: &str) -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        // Create .agent-doc/ directory structure (project root marker)
        fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/baselines")).unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/pending")).unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/crdt")).unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/pre-response")).unwrap();

        let doc = dir.path().join("test.md");
        let content = format!(
            "---\nagent_doc_session: {}\nagent_doc_format: template\n---\n\n## Exchange\nSome content\n",
            session_uuid
        );
        fs::write(&doc, &content).unwrap();
        (dir, doc)
    }

    #[test]
    fn try_migrate_renamed_finds_orphaned_snapshot() {
        let session_uuid = "test-uuid-1234";
        let (dir, old_doc) = setup_project_with_session(session_uuid);

        // Save a snapshot for the old path
        let old_hash = doc_hash(&old_doc).unwrap();
        let snap_dir = dir.path().join(".agent-doc/snapshots");
        let old_snap_content = format!(
            "---\nagent_doc_session: {}\nagent_doc_format: template\n---\n\nStripped exchange\n",
            session_uuid
        );
        fs::write(snap_dir.join(format!("{}.md", old_hash)), &old_snap_content).unwrap();

        // Also create a baseline and pending file for the old hash
        let baseline_dir = dir.path().join(".agent-doc/baselines");
        fs::write(
            baseline_dir.join(format!("{}.md", old_hash)),
            "baseline content",
        )
        .unwrap();
        let pending_dir = dir.path().join(".agent-doc/pending");
        fs::write(
            pending_dir.join(format!("{}.md", old_hash)),
            "pending content",
        )
        .unwrap();

        // "Rename" the file
        let new_doc = dir.path().join("renamed.md");
        fs::rename(&old_doc, &new_doc).unwrap();

        // Verify new hash differs
        let new_hash = doc_hash(&new_doc).unwrap();
        assert_ne!(old_hash, new_hash);

        // Run migration
        let migrated = try_migrate_renamed(&new_doc).unwrap();
        assert!(migrated, "should detect and migrate orphaned snapshot");

        // Verify state files were migrated
        assert!(snap_dir.join(format!("{}.md", new_hash)).exists());
        assert!(!snap_dir.join(format!("{}.md", old_hash)).exists());
        assert!(baseline_dir.join(format!("{}.md", new_hash)).exists());
        assert!(!baseline_dir.join(format!("{}.md", old_hash)).exists());
        assert!(pending_dir.join(format!("{}.md", new_hash)).exists());
        assert!(!pending_dir.join(format!("{}.md", old_hash)).exists());

        // Verify snapshot content preserved
        let new_snap = fs::read_to_string(snap_dir.join(format!("{}.md", new_hash))).unwrap();
        assert_eq!(new_snap, old_snap_content);
    }

    #[test]
    fn try_migrate_renamed_noop_when_snapshot_exists() {
        let session_uuid = "test-uuid-5678";
        let (dir, doc) = setup_project_with_session(session_uuid);

        // Save a snapshot for the current path
        let hash = doc_hash(&doc).unwrap();
        let snap_dir = dir.path().join(".agent-doc/snapshots");
        fs::write(snap_dir.join(format!("{}.md", hash)), "existing snapshot").unwrap();

        let migrated = try_migrate_renamed(&doc).unwrap();
        assert!(!migrated, "should not migrate when snapshot already exists");
    }

    #[test]
    fn try_migrate_renamed_noop_when_no_session() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(
            &doc,
            "---\nagent_doc_format: template\n---\n\nNo session UUID\n",
        )
        .unwrap();

        let migrated = try_migrate_renamed(&doc).unwrap();
        assert!(!migrated, "should not migrate without session UUID");
    }

    #[test]
    fn try_migrate_renamed_noop_when_no_match() {
        let session_uuid = "test-uuid-abcd";
        let (dir, doc) = setup_project_with_session(session_uuid);

        // Create a snapshot with a DIFFERENT session UUID
        let snap_dir = dir.path().join(".agent-doc/snapshots");
        fs::write(
            snap_dir.join("deadbeef1234567890abcdef1234567890abcdef1234567890abcdef12345678.md"),
            "---\nagent_doc_session: different-uuid\nagent_doc_format: template\n---\n\nOther doc\n",
        ).unwrap();

        let migrated = try_migrate_renamed(&doc).unwrap();
        assert!(!migrated, "should not migrate when no matching UUID found");
    }

    #[test]
    fn ensure_initialized_assigns_uuid_even_when_snapshot_exists() {
        let (dir, doc) = setup_with_frontmatter(
            "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\nBody\n",
        );
        // Pre-create snapshot so ensure_snapshot is a no-op
        fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        write_snapshot_directly(dir.path(), &doc, "pre-existing snapshot");

        let initialized = ensure_initialized(&doc).unwrap();
        assert!(initialized, "should return true when UUID was assigned");

        let content = fs::read_to_string(&doc).unwrap();
        assert!(
            content.contains("agent_doc_session:"),
            "UUID should be assigned"
        );
    }

    /// Simulates the `start` command scenario: file already has a session UUID
    /// (written by `frontmatter::ensure_session`), then is moved. Calling
    /// `ensure_initialized` on the new path should migrate orphaned state.
    #[test]
    fn ensure_initialized_migrates_after_move_with_existing_session() {
        let session_uuid = "start-scenario-uuid-1234";
        let (dir, old_doc) = setup_project_with_session(session_uuid);

        // Create snapshot + CRDT state for old path (simulates prior session)
        let old_hash = doc_hash(&old_doc).unwrap();
        let snap_dir = dir.path().join(".agent-doc/snapshots");
        let crdt_dir = dir.path().join(".agent-doc/crdt");
        let old_snap = format!(
            "---\nagent_doc_session: {}\nagent_doc_format: template\n---\n\nOld snapshot\n",
            session_uuid
        );
        fs::write(snap_dir.join(format!("{}.md", old_hash)), &old_snap).unwrap();
        fs::write(crdt_dir.join(format!("{}.yrs", old_hash)), b"crdt-state").unwrap();
        fs::write(
            crdt_dir.join(format!("{}.overlay.yrs", old_hash)),
            b"overlay-crdt-state",
        )
        .unwrap();

        // Move the file (simulates JB plugin respawn after rename)
        let new_doc = dir.path().join("moved-doc.md");
        fs::rename(&old_doc, &new_doc).unwrap();
        let new_hash = doc_hash(&new_doc).unwrap();
        assert_ne!(old_hash, new_hash);

        // Call ensure_initialized — the path start.rs now takes
        let initialized = ensure_initialized(&new_doc).unwrap();
        assert!(initialized, "should migrate orphaned state");

        // Snapshot migrated to new hash
        assert!(snap_dir.join(format!("{}.md", new_hash)).exists());
        assert!(!snap_dir.join(format!("{}.md", old_hash)).exists());

        // CRDT state migrated to new hash
        assert!(crdt_dir.join(format!("{}.yrs", new_hash)).exists());
        assert!(!crdt_dir.join(format!("{}.yrs", old_hash)).exists());
        assert!(crdt_dir.join(format!("{}.overlay.yrs", new_hash)).exists());
        assert!(!crdt_dir.join(format!("{}.overlay.yrs", old_hash)).exists());
    }

    /// When `start` calls `ensure_initialized` on a file with no prior state
    /// (no snapshot, no orphaned state), it should bootstrap a fresh snapshot.
    #[test]
    fn ensure_initialized_bootstraps_snapshot_for_fresh_file() {
        let session_uuid = "fresh-start-uuid-5678";
        let (dir, doc) = setup_project_with_session(session_uuid);

        // No snapshot exists — ensure_initialized should create one
        let initialized = ensure_initialized(&doc).unwrap();
        assert!(initialized, "should create snapshot for fresh file");

        // Verify snapshot was created
        let hash = doc_hash(&doc).unwrap();
        let snap_dir = dir.path().join(".agent-doc/snapshots");
        assert!(
            snap_dir.join(format!("{}.md", hash)).exists(),
            "snapshot should be bootstrapped"
        );
    }

    // -----------------------------------------------------------------------
    // `#mps` Rung 1 — byte-stable projection evals
    //
    // Prove the structured overlay model projects each document shape agent-doc
    // actually persists back to byte-identical markdown through the same
    // encode→decode→to_markdown pipeline the merge base uses. These are the
    // offline half of the #mps obstacle-2 gate; the env-gated `save` probe
    // collects the live-traffic half.
    // -----------------------------------------------------------------------

    fn assert_projection_byte_stable(label: &str, content: &str) {
        let projected = project_overlay_roundtrip(content);
        assert_eq!(
            projected.as_deref(),
            Some(content),
            "#mps overlay projection not byte-stable for {label}"
        );
        assert!(overlay_projection_is_byte_stable(content), "{label}");
    }

    #[test]
    fn mps_projection_byte_stable_inline_shape() {
        let inline = concat!(
            "---\nagent_doc_format: inline\n---\n\n",
            "## User\n\nDo the thing.\n\n",
            "## Assistant\n\nDid the thing.\n"
        );
        assert_projection_byte_stable("inline", inline);
    }

    #[test]
    fn mps_projection_byte_stable_template_queue_shape() {
        let template = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "## Queue\n\n<!-- agent:queue -->\n",
            "- do [#a]\n- do [#b]\n",
            "<!-- /agent:queue -->\n"
        );
        assert_projection_byte_stable("template_queue", template);
    }

    #[test]
    fn mps_projection_byte_stable_exchange_append_shape() {
        let exchange = concat!(
            "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "## Exchange\n\n<!-- agent:exchange patch=append -->\n",
            "\u{276f} first question\n\n",
            "### Re: first question \u{2014} opus-4-8\n\n",
            "First answer here.\n\n",
            "\u{276f} second question\n",
            "<!-- /agent:exchange -->\n"
        );
        assert_projection_byte_stable("exchange_append", exchange);
    }

    #[test]
    fn mps_projection_byte_stable_boundary_marker_shape() {
        let with_boundary = concat!(
            "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "## Exchange\n\n<!-- agent:exchange patch=append -->\n",
            "\u{276f} q\n\n### Re: q \u{2014} opus-4-8\n\nA.\n\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n"
        );
        assert_projection_byte_stable("boundary_marker", with_boundary);
    }

    #[test]
    fn mps_projection_byte_stable_empty_and_unicode() {
        assert_projection_byte_stable("empty", "");
        assert_projection_byte_stable("plain", "just a line of prose\n");
        assert_projection_byte_stable(
            "unicode",
            "# T\u{e9}l\u{e9} \u{1f680}\n\n- caf\u{e9} \u{2014} r\u{e9}sum\u{e9}\n",
        );
    }

    // -----------------------------------------------------------------------
    // `#mps` Rungs 2-4 — model-projected baseline store
    // -----------------------------------------------------------------------

    #[test]
    fn mps_baseline_model_roundtrips_byte_identical() {
        let (_dir, doc) = setup();
        let baseline = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "## Queue\n\n<!-- agent:queue -->\n- do [#a]\n- do [#b]\n<!-- /agent:queue -->\n"
        );
        save_baseline_model(&doc, baseline).unwrap();
        // Cross-checked against the matching `.md` content: projects identically,
        // no divergence.
        let projected = load_baseline_model(&doc, Some(baseline)).unwrap();
        assert_eq!(projected.as_deref(), Some(baseline));
    }

    #[test]
    fn mps_baseline_model_none_when_absent() {
        let (_dir, doc) = setup();
        // No sidecar pinned → caller falls back to the `.md` baseline.
        assert!(load_baseline_model(&doc, Some("anything")).unwrap().is_none());
    }

    #[test]
    fn mps_baseline_model_prefers_md_backstop_on_divergence() {
        let (_dir, doc) = setup();
        let pinned = "## Queue\n<!-- agent:queue -->\n- do [#real]\n<!-- /agent:queue -->\n";
        save_baseline_model(&doc, pinned).unwrap();
        // When the `.md` cross-check cache disagrees with the model projection, the
        // proven `.md` wins as the non-regressing backstop (until Rung 4 removes
        // it); the divergence is logged. This is what makes default-on safe.
        let stale_md = "## Queue\n<!-- agent:queue -->\n- do [#stale]\n<!-- /agent:queue -->\n";
        let resolved = load_baseline_model(&doc, Some(stale_md)).unwrap();
        assert_eq!(resolved.as_deref(), Some(stale_md));
    }

    #[test]
    fn mps_baseline_model_uses_projection_when_no_md() {
        let (_dir, doc) = setup();
        // With no `.md` (the eventual Rung-4 state), the model projection is the
        // only and authoritative base.
        let pinned = "## Queue\n<!-- agent:queue -->\n- do [#only]\n<!-- /agent:queue -->\n";
        save_baseline_model(&doc, pinned).unwrap();
        let resolved = load_baseline_model(&doc, None).unwrap();
        assert_eq!(resolved.as_deref(), Some(pinned));
    }

    #[test]
    fn mps_delete_baseline_model_idempotent() {
        let (_dir, doc) = setup();
        // Delete-before-create is a no-op, not an error.
        delete_baseline_model(&doc).unwrap();
        save_baseline_model(&doc, "x\n").unwrap();
        assert!(baseline_overlay_path_for(&doc).unwrap().exists());
        delete_baseline_model(&doc).unwrap();
        assert!(!baseline_overlay_path_for(&doc).unwrap().exists());
        delete_baseline_model(&doc).unwrap();
    }

    #[test]
    fn mps_first_diff_byte_locates_mismatch() {
        assert_eq!(first_diff_byte("abc", "abc"), None);
        assert_eq!(first_diff_byte("abc", "abx"), Some(2));
        assert_eq!(first_diff_byte("abc", "ab"), Some(2)); // prefix → shorter len
        assert_eq!(first_diff_byte("ab", "abcd"), Some(2));
    }
