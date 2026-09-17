use std::path::Path;

fn main() {
    // The reliable-sync registration is the generation authority used by every
    // native editor effect. Bake all editor package generations here so callers
    // cannot disagree about what "current" means.
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let editors = Path::new(&manifest_dir).join("../editors");

    let jb_props = editors.join("jetbrains/gradle.properties");
    emit(
        "AGENT_DOC_EXPECTED_JETBRAINS_PLUGIN_VERSION",
        read_property(&jb_props, "pluginVersion"),
    );
    println!("cargo:rerun-if-changed={}", jb_props.display());

    let vscode_pkg = editors.join("vscode/package.json");
    emit(
        "AGENT_DOC_EXPECTED_VSCODE_PLUGIN_VERSION",
        read_json_string(&vscode_pkg, "version"),
    );
    println!("cargo:rerun-if-changed={}", vscode_pkg.display());

    let zed_manifest = editors.join("zed/extension.toml");
    emit(
        "AGENT_DOC_EXPECTED_ZED_PLUGIN_VERSION",
        read_property(&zed_manifest, "version"),
    );
    println!("cargo:rerun-if-changed={}", zed_manifest.display());
}

fn emit(name: &str, value: Option<String>) {
    if let Some(value) = value {
        println!("cargo:rustc-env={name}={value}");
    }
}

fn read_property(path: &Path, key: &str) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix(key)
            && let Some(value) = rest.trim_start().strip_prefix('=')
        {
            let value = value.trim().trim_matches('"');
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn read_json_string(path: &Path, key: &str) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let needle = format!("\"{key}\"");
    let after = &content[content.find(&needle)? + needle.len()..];
    let after_colon = &after[after.find(':')? + 1..];
    let rest = &after_colon[after_colon.find('"')? + 1..];
    let value = &rest[..rest.find('"')?];
    (!value.is_empty()).then(|| value.to_string())
}
