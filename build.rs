fn main() {
    // Embed build timestamp so the binary can detect stale instances
    println!(
        "cargo:rustc-env=AGENT_DOC_BUILD_TIMESTAMP={}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    );
    // Re-run build.rs on every build (not just when build.rs changes)
    println!("cargo:rerun-if-changed=src/");
}
