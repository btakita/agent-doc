use std::fs;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("Cargo.toml"),
        "[workspace]\nmembers = []\n",
    )
    .unwrap();
    for path in [
        "target/debug",
        "editors/jetbrains/build/distributions",
        ".agent-doc-build-worker",
        ".cargo/registry",
        ".tsift/artifacts",
        "target-cache",
    ] {
        fs::create_dir_all(root.path().join(path)).unwrap();
    }
    root
}

#[test]
fn release_cleanup_removes_only_repo_owned_build_outputs() {
    let root = fixture();
    let status = Command::new("bash")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/cleanup-build-artifacts.sh"
        ))
        .args(["--root", root.path().to_str().unwrap()])
        .status()
        .unwrap();
    assert!(status.success());

    for removed in [
        "target",
        "editors/jetbrains/build",
        ".agent-doc-build-worker",
    ] {
        assert!(
            !root.path().join(removed).exists(),
            "{removed} was retained"
        );
    }
    for preserved in [".cargo/registry", ".tsift/artifacts", "target-cache"] {
        assert!(
            root.path().join(preserved).exists(),
            "{preserved} was removed"
        );
    }
}

#[test]
fn release_cleanup_can_be_disabled_for_incremental_build_caching() {
    let root = fixture();
    let status = Command::new("bash")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/cleanup-build-artifacts.sh"
        ))
        .args(["--root", root.path().to_str().unwrap()])
        .env("AGENT_DOC_CLEAN_BUILD_ARTIFACTS", "0")
        .status()
        .unwrap();
    assert!(status.success());
    assert!(root.path().join("target").exists());
    assert!(root.path().join("editors/jetbrains/build").exists());
}

#[test]
fn release_cleanup_detaches_a_build_tree_from_concurrent_writers() {
    let root = fixture();
    let original = root.path().join("target/debug/original-generation");
    fs::write(&original, "old build").unwrap();
    let stop = root.path().join("stop-writer");
    let ready = root.path().join("writer-ready");
    let writer_script = r#"
set -u
root="$1"
touch "$root/writer-ready"
i=0
while [ ! -e "$root/stop-writer" ]; do
  mkdir -p "$root/target/debug/deps" 2>/dev/null || true
  touch "$root/target/debug/deps/recreated-$i" 2>/dev/null || true
  i=$((i + 1))
done
"#;
    let mut writer = Command::new("bash")
        .args([
            "-c",
            writer_script,
            "cleanup-writer",
            root.path().to_str().unwrap(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let ready_deadline = Instant::now() + Duration::from_secs(2);
    while !ready.exists() && Instant::now() < ready_deadline {
        thread::sleep(Duration::from_millis(5));
    }
    assert!(ready.exists(), "concurrent build writer did not start");

    let status = Command::new("bash")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/cleanup-build-artifacts.sh"
        ))
        .args(["--root", root.path().to_str().unwrap()])
        .status()
        .unwrap();
    fs::write(&stop, "stop").unwrap();
    let writer_status = writer.wait().unwrap();

    assert!(status.success());
    assert!(writer_status.success());
    assert!(
        !original.exists(),
        "the detached build generation was retained"
    );
    let cleanup_generations = fs::read_dir(root.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".agent-doc-build-cleanup.")
        })
        .count();
    assert_eq!(cleanup_generations, 0, "detached cleanup generation leaked");
}
