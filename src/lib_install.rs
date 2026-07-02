use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub(crate) fn platform_lib_name() -> &'static str {
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

pub fn run(source: Option<&str>, target_dir: Option<&str>, profile: &str) -> Result<()> {
    let source_path = source.map(PathBuf::from);
    let target_dir = target_dir.map(PathBuf::from);
    run_paths(source_path.as_deref(), target_dir.as_deref(), profile)
}

pub(crate) fn run_paths(
    source: Option<&Path>,
    target_dir: Option<&Path>,
    profile: &str,
) -> Result<()> {
    let version = env!("CARGO_PKG_VERSION");
    let ext = platform_lib_ext();
    let profile = normalized_profile(profile);

    let source_path = match source {
        Some(s) => s.to_path_buf(),
        None => {
            let cwd = std::env::current_dir()?;
            let cargo_target_dir = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from);
            profile_lib_path(&cwd, cargo_target_dir.as_deref(), profile)
        }
    };

    if !source_path.exists() {
        anyhow::bail!(
            "[lib-install] source not found: {}\nBuild with: cargo build --profile {} --lib",
            source_path.display(),
            profile
        );
    }
    validate_required_editor_abi_symbols(&source_path)?;

    let target = match target_dir {
        Some(d) => d.to_path_buf(),
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
    // #autorecycle-on-install (upgrades #ctlrecycle R4 from print-only to action):
    // the JetBrains plugin hot-reloads this cdylib by mtime, but already-running
    // agent-doc controllers/supervisors keep serving the PRIOR binary until they
    // recycle. Instead of only printing the recycle hint, automatically mark every
    // running controller to recycle at its next idle boundary so the new build goes
    // live everywhere, not just in the editor cdylib. The recycle is idle-gated
    // (`recycle_controllers_all_projects` sends a `recycle` RPC that fires only at a
    // turn/inter-queue-item boundary, never mid-turn), so triggering it from
    // `lib-install` is safe. Opt out with a falsey AGENT_DOC_RECYCLE_ON_INSTALL.
    auto_recycle_after_install();

    Ok(())
}

fn normalized_profile(profile: &str) -> &str {
    match profile.trim() {
        "" => "release",
        trimmed => trimmed,
    }
}

pub(crate) fn profile_lib_path(
    cwd: &Path,
    cargo_target_dir: Option<&Path>,
    profile: &str,
) -> PathBuf {
    let target_root = cargo_target_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| cwd.join("target"));
    target_root
        .join(normalized_profile(profile))
        .join(platform_lib_name())
}

const REQUIRED_EDITOR_ABI_SYMBOLS: &[&str] = &[
    "agent_doc_document_changed_digest_content_for_editor_v2",
    "agent_doc_document_changed_digest_content_for_editor_v3",
    "agent_doc_document_synced_digest_content_for_editor_v2",
    "agent_doc_write_ack_content_for_editor_v2",
    "agent_doc_document_closed_for_editor",
];

fn validate_required_editor_abi_symbols(source: &Path) -> Result<()> {
    let bytes = std::fs::read(source)
        .with_context(|| format!("read shared library {}", source.display()))?;
    let missing: Vec<&str> = REQUIRED_EDITOR_ABI_SYMBOLS
        .iter()
        .copied()
        .filter(|symbol| !contains_ascii_symbol(&bytes, symbol.as_bytes()))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    anyhow::bail!(
        "[lib-install] source {} is missing required editor authority ABI symbol(s): {}. Rebuild the current checkout with `cargo build --lib` before installing.",
        source.display(),
        missing.join(", ")
    );
}

fn contains_ascii_symbol(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

/// `#autorecycle-on-install`: default-on resolution for auto-recycling running
/// controllers after a `lib-install`. Falsey `AGENT_DOC_RECYCLE_ON_INSTALL`
/// (`0`/`false`/`no`/`off`) opts out and restores the print-only hint.
fn recycle_on_install_enabled() -> bool {
    match std::env::var("AGENT_DOC_RECYCLE_ON_INSTALL") {
        Ok(v) => !matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "no" | "off"
        ),
        Err(_) => true,
    }
}

/// Mark every running controller to recycle onto the freshly-installed binary at its
/// next idle boundary. Best-effort: a recycle failure must never fail the install, so
/// errors are logged (never swallowed silently) and the print-only hint is surfaced as
/// a fallback. When opted out, only the hint is printed.
fn auto_recycle_after_install() {
    if !recycle_on_install_enabled() {
        eprintln!(
            "[lib-install] note: auto-recycle opted out (AGENT_DOC_RECYCLE_ON_INSTALL falsey) — running controllers still serve the prior binary; run `agent-doc admin recycle --all-projects` (or restart sessions) to promote the new build"
        );
        return;
    }
    match agent_doc_orchestration::project_controller::recycle_controllers_all_projects() {
        Ok((recycled, skipped)) => {
            eprintln!(
                "[lib-install] auto-recycle: {recycled} controller(s) marked to recycle at next idle boundary, {skipped} skipped (set AGENT_DOC_RECYCLE_ON_INSTALL=0 to disable)"
            );
        }
        Err(e) => {
            eprintln!(
                "[lib-install] warning: auto-recycle failed ({e}) — running controllers still serve the prior binary; run `agent-doc admin recycle --all-projects` (or restart sessions) to promote the new build"
            );
        }
    }
    // `#turnsaferecycle` Goal 1 — controllers (PCPs) are only half the fleet. The
    // long-lived `agent-doc start --route-owned` supervisors that actually write
    // documents must also be marked to recycle onto the freshly-installed binary,
    // otherwise they keep serving the prior build until each independently
    // self-detects staleness.
    match agent_doc_orchestration::project_controller::recycle_supervisors_all_projects() {
        Ok((marked, skipped)) => {
            eprintln!(
                "[lib-install] auto-recycle: {marked} route-owned supervisor(s) marked to recycle at next idle boundary, {skipped} skipped"
            );
        }
        Err(e) => {
            eprintln!(
                "[lib-install] warning: supervisor recycle fan-out failed ({e}) — route-owned supervisors still serve the prior binary until they self-detect staleness"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn recycle_on_install_default_on_and_opt_out_is_falsey() {
        // #autorecycle-on-install: default-on, falsey env opts out. Serialize env
        // mutation to avoid cross-test interference.
        let prior = std::env::var("AGENT_DOC_RECYCLE_ON_INSTALL").ok();
        unsafe { std::env::remove_var("AGENT_DOC_RECYCLE_ON_INSTALL") };
        assert!(recycle_on_install_enabled(), "default must be ON");
        for falsey in ["0", "false", "no", "off", "OFF", " False "] {
            unsafe { std::env::set_var("AGENT_DOC_RECYCLE_ON_INSTALL", falsey) };
            assert!(
                !recycle_on_install_enabled(),
                "falsey value {falsey:?} must opt out"
            );
        }
        for truthy in ["1", "true", "yes", "on", "anything"] {
            unsafe { std::env::set_var("AGENT_DOC_RECYCLE_ON_INSTALL", truthy) };
            assert!(
                recycle_on_install_enabled(),
                "truthy/other value {truthy:?} stays ON"
            );
        }
        unsafe {
            match prior {
                Some(v) => std::env::set_var("AGENT_DOC_RECYCLE_ON_INSTALL", v),
                None => std::env::remove_var("AGENT_DOC_RECYCLE_ON_INSTALL"),
            }
        }
    }

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
    fn lib_install_rejects_source_missing_editor_authority_abi() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join(platform_lib_name());
        fs::write(&source, b"fake library content").unwrap();

        let err = validate_required_editor_abi_symbols(&source).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("missing required editor authority ABI"));
        assert!(message.contains("agent_doc_document_changed_digest_content_for_editor_v2"));
    }

    #[test]
    fn lib_install_accepts_source_with_editor_authority_abi() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join(platform_lib_name());
        fs::write(
            &source,
            REQUIRED_EDITOR_ABI_SYMBOLS
                .iter()
                .copied()
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();

        validate_required_editor_abi_symbols(&source).unwrap();
    }

    #[test]
    fn profile_lib_path_uses_profile_and_cargo_target_dir() {
        let cwd = PathBuf::from("/tmp/agent-doc");

        assert_eq!(
            profile_lib_path(&cwd, None, "release-local"),
            cwd.join("target")
                .join("release-local")
                .join(platform_lib_name())
        );
        assert_eq!(
            profile_lib_path(
                &cwd,
                Some(Path::new("target/local-install")),
                "release-local"
            ),
            PathBuf::from("target/local-install")
                .join("release-local")
                .join(platform_lib_name())
        );
        assert_eq!(
            profile_lib_path(&cwd, None, " "),
            cwd.join("target").join("release").join(platform_lib_name())
        );
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
