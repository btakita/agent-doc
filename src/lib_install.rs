use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

fn platform_lib_name() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "libagent_doc.so"
    }
    #[cfg(target_os = "macos")]
    {
        "libagent_doc.dylib"
    }
    #[cfg(target_os = "windows")]
    {
        "agent_doc.dll"
    }
}

fn platform_lib_ext() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "so"
    }
    #[cfg(target_os = "macos")]
    {
        "dylib"
    }
    #[cfg(target_os = "windows")]
    {
        "dll"
    }
}

pub fn versioned_lib_name(version: &str) -> String {
    #[cfg(target_os = "linux")]
    {
        format!("libagent_doc-{}.so", version)
    }
    #[cfg(target_os = "macos")]
    {
        format!("libagent_doc-{}.dylib", version)
    }
    #[cfg(target_os = "windows")]
    {
        format!("agent_doc-{}.dll", version)
    }
}

pub fn install_versioned(source: &Path, target_dir: &Path, version: &str) -> Result<PathBuf> {
    let versioned = versioned_lib_name(version);
    let dst = target_dir.join(&versioned);
    let symlink = target_dir.join(platform_lib_name());
    let tmp_symlink = target_dir.join(format!("{}.tmp", platform_lib_name()));

    // Atomic file replace: copy to temp, then rename. This creates a new inode
    // so any existing mmap (e.g., IDEA's FFI handle) remains valid on the old inode.
    let tmp_dst = target_dir.join(format!(".{}.tmp", versioned));
    std::fs::copy(source, &tmp_dst)
        .with_context(|| format!("copy {} -> {}", source.display(), tmp_dst.display()))?;
    std::fs::rename(&tmp_dst, &dst)
        .with_context(|| format!("rename {} -> {}", tmp_dst.display(), dst.display()))?;

    // Atomic symlink swap: create temp symlink, then rename over the real one.
    // Use relative target so the symlink works if the directory is moved.
    if tmp_symlink.exists() {
        std::fs::remove_file(&tmp_symlink)?;
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&versioned, &tmp_symlink)
        .with_context(|| format!("symlink {} -> {}", tmp_symlink.display(), versioned))?;
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&versioned, &tmp_symlink)
        .with_context(|| format!("symlink {} -> {}", tmp_symlink.display(), versioned))?;

    std::fs::rename(&tmp_symlink, &symlink)
        .with_context(|| format!("rename {} -> {}", tmp_symlink.display(), symlink.display()))?;

    Ok(dst)
}

pub fn run(source: Option<&str>, target_dir: Option<&str>) -> Result<()> {
    let version = env!("CARGO_PKG_VERSION");
    let ext = platform_lib_ext();

    let source_path = match source {
        Some(s) => PathBuf::from(s),
        None => {
            let cwd = std::env::current_dir()?;
            cwd.join(format!("target/release/{}", platform_lib_name()))
        }
    };

    if !source_path.exists() {
        anyhow::bail!(
            "[lib-install] source not found: {}\nBuild with: cargo build --release --lib",
            source_path.display()
        );
    }

    let target = match target_dir {
        Some(d) => PathBuf::from(d),
        None => {
            let exe = std::env::current_exe()?;
            exe.parent()
                .context("cannot determine binary directory")?
                .to_path_buf()
        }
    };

    let installed = install_versioned(&source_path, &target, version)?;
    eprintln!(
        "[lib-install] {} -> {} (symlink: libagent_doc.{})",
        source_path.display(),
        installed.display(),
        ext,
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn versioned_name_includes_version() {
        let name = versioned_lib_name("0.33.4");
        assert!(name.contains("0.33.4"));
        assert!(name.starts_with("libagent_doc-") || name.starts_with("agent_doc-"));
    }

    #[test]
    fn install_creates_versioned_file_and_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join(platform_lib_name());
        fs::write(&source, b"fake library content").unwrap();

        let target_dir = tmp.path().join("install");
        fs::create_dir(&target_dir).unwrap();

        let installed = install_versioned(&source, &target_dir, "1.2.3").unwrap();

        // Versioned file exists with correct content
        assert!(installed.exists());
        assert_eq!(fs::read(&installed).unwrap(), b"fake library content");
        assert!(
            installed
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .contains("1.2.3")
        );

        // Symlink exists and resolves to the versioned file
        let symlink = target_dir.join(platform_lib_name());
        assert!(symlink.exists());
        assert!(symlink.is_symlink());
        let target = fs::read_link(&symlink).unwrap();
        assert_eq!(target.to_str().unwrap(), versioned_lib_name("1.2.3"));
    }

    #[test]
    fn install_swaps_symlink_on_version_change() {
        let tmp = tempfile::tempdir().unwrap();
        let target_dir = tmp.path().join("install");
        fs::create_dir(&target_dir).unwrap();

        // First version
        let source_v1 = tmp.path().join("v1.so");
        fs::write(&source_v1, b"v1 content").unwrap();
        install_versioned(&source_v1, &target_dir, "1.0.0").unwrap();

        let symlink = target_dir.join(platform_lib_name());
        assert_eq!(
            fs::read_link(&symlink).unwrap().to_str().unwrap(),
            versioned_lib_name("1.0.0")
        );

        // Second version
        let source_v2 = tmp.path().join("v2.so");
        fs::write(&source_v2, b"v2 content").unwrap();
        install_versioned(&source_v2, &target_dir, "2.0.0").unwrap();

        // Symlink now points to v2
        assert_eq!(
            fs::read_link(&symlink).unwrap().to_str().unwrap(),
            versioned_lib_name("2.0.0")
        );

        // v1 versioned file still exists (for GC later)
        assert!(target_dir.join(versioned_lib_name("1.0.0")).exists());

        // Symlink resolves to v2 content
        assert_eq!(fs::read(&symlink).unwrap(), b"v2 content");
    }

    #[test]
    fn same_version_reinstall_creates_new_inode() {
        use std::os::unix::fs::MetadataExt;

        let tmp = tempfile::tempdir().unwrap();
        let target_dir = tmp.path().join("install");
        fs::create_dir(&target_dir).unwrap();

        let source_v1 = tmp.path().join("v1.so");
        fs::write(&source_v1, b"original content").unwrap();
        install_versioned(&source_v1, &target_dir, "1.0.0").unwrap();

        let versioned_path = target_dir.join(versioned_lib_name("1.0.0"));
        let ino_before = fs::metadata(&versioned_path).unwrap().ino();

        // Reinstall same version with different content
        let source_v1b = tmp.path().join("v1b.so");
        fs::write(&source_v1b, b"updated content").unwrap();
        install_versioned(&source_v1b, &target_dir, "1.0.0").unwrap();

        let ino_after = fs::metadata(&versioned_path).unwrap().ino();

        // Atomic rename must produce a new inode — old mmap stays valid on old inode
        assert_ne!(
            ino_before, ino_after,
            "same-version reinstall must create new inode"
        );
        assert_eq!(fs::read(&versioned_path).unwrap(), b"updated content");
    }

    #[test]
    fn install_replaces_regular_file_with_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let target_dir = tmp.path().join("install");
        fs::create_dir(&target_dir).unwrap();

        // Pre-existing unversioned file (legacy install)
        let legacy = target_dir.join(platform_lib_name());
        fs::write(&legacy, b"old unversioned").unwrap();
        assert!(!legacy.is_symlink());

        let source = tmp.path().join("new.so");
        fs::write(&source, b"versioned content").unwrap();
        install_versioned(&source, &target_dir, "3.0.0").unwrap();

        // Now it's a symlink
        assert!(legacy.is_symlink());
        assert_eq!(fs::read(&legacy).unwrap(), b"versioned content");
    }
}
