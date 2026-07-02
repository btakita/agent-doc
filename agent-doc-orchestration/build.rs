use std::path::Path;

fn main() {
    // Embed build timestamp so the binary can detect stale instances
    println!(
        "cargo:rustc-env=AGENT_DOC_BUILD_TIMESTAMP={}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    );

    // `#stale-plugin-detect`: bake the expected editor-plugin versions from their
    // source-of-truth files so the running binary can warn when a *live* plugin is
    // older than the build this binary ships with. Fail-open: when the editor
    // sources are absent (e.g. a crates.io build without the `editors/` tree) we
    // omit the env var and `option_env!` yields `None` — no stale-plugin warning,
    // no false positive.
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let editors = Path::new(&manifest_dir).join("../editors");

    let jb_props = editors.join("jetbrains/gradle.properties");
    if let Some(version) = read_gradle_property(&jb_props, "pluginVersion") {
        println!("cargo:rustc-env=AGENT_DOC_EXPECTED_JETBRAINS_PLUGIN_VERSION={version}");
    }
    println!("cargo:rerun-if-changed={}", jb_props.display());

    let vscode_pkg = editors.join("vscode/package.json");
    if let Some(version) = read_json_top_level_string(&vscode_pkg, "version") {
        println!("cargo:rustc-env=AGENT_DOC_EXPECTED_VSCODE_PLUGIN_VERSION={version}");
    }
    println!("cargo:rerun-if-changed={}", vscode_pkg.display());

    // Re-run build.rs on every build (not just when build.rs changes)
    println!("cargo:rerun-if-changed=src/");
}

/// Read a `key = value` line from a Java-style `.properties` file.
fn read_gradle_property(path: &Path, key: &str) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix(key)
            && let Some(value) = rest.trim_start().strip_prefix('=')
        {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// Minimal dependency-free extraction of the first top-level `"key": "value"`
/// string from a JSON file. Build scripts avoid pulling serde into the build
/// graph; `package.json`'s top-level `version` is the first `"version"` key.
fn read_json_top_level_string(path: &Path, key: &str) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let needle = format!("\"{key}\"");
    let idx = content.find(&needle)?;
    let after = &content[idx + needle.len()..];
    let colon = after.find(':')?;
    let after_colon = &after[colon + 1..];
    let start = after_colon.find('"')? + 1;
    let rest = &after_colon[start..];
    let end = rest.find('"')?;
    let value = &rest[..end];
    (!value.is_empty()).then(|| value.to_string())
}
