//! Grok's adapter uses the shared template. agent-kit has no Grok Environment
//! yet, so this additional target remains in the existing product installer.
use super::*;

const REL_PATH: &str = ".grok/skills/agent-doc";

fn content() -> String {
    let rendered = render_skill(
        agent_kit::detect::Environment::Generic,
        "Interactive markdown session. TRIGGER: agent-doc <file>. Requires binary admission and committed closeout.",
        "## Invocation\n\nSubmit `agent-doc <FILE>` in Grok Build. Read this skill, call connected `agent_doc_admit` for FILE, then read the document and run `agent-doc plan <FILE>`. Never recursively launch `agent-doc <FILE>` from a shell.\n\nGrok lifecycle hooks are notifications: stdout cannot inject admission context or continue the turn. Successful MCP admission is the required checkpoint. Use connected `agent_doc_finalize` for closeout. If MCP tools are absent, install with `agent-doc skill install --harness grok`, then enable/trust the project MCP server in Grok. Never bypass refused admission.\n\nAfter a committed finalize, follow its queue-continuation fields in the same turn, admitting and completing each next actionable head. Do not end while a drainable head remains. Do not run another session-check after a successful connected finalize.",
    );
    let mut result = String::new();
    for line in rendered.lines() {
        if line.starts_with("- **Preflight is binary-owned")
            || line.starts_with("**Preflight runs in the binary")
        {
            result.push_str("**Admission is binary-owned:** call connected `agent_doc_admit` before answering each live prompt. Proceed only if admitted; report refusals without recreating admission or shelling preflight. Read with `agent-doc read <FILE>` and use `agent-doc plan <FILE>` for the execution contract.\n");
        } else if line.starts_with("**Auto-update skill:**") {
            result.push_str("**Auto-update skill:** if the binary is newer, run `agent-doc skill install --harness grok`, re-read `.grok/skills/agent-doc/SKILL.md`, and continue this turn.\n");
        } else {
            result.push_str(line);
            result.push('\n');
        }
    }
    result
}

fn install_bundle(dir: &Path, bundle: &[(&str, &str)]) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    for (name, text) in bundle {
        write_if_changed(&dir.join(name), text)?;
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && entry.path().extension().is_some_and(|ext| ext == "md")
            && !bundle.iter().any(|(name, _)| entry.file_name() == *name)
        {
            std::fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

pub fn install(root: Option<&Path>) -> Result<()> {
    let base = root
        .map(Path::to_path_buf)
        .or_else(resolve_root)
        .unwrap_or(std::env::current_dir()?);
    let dir = base.join(REL_PATH);
    std::fs::create_dir_all(&dir)?;
    write_if_changed(&dir.join("SKILL.md"), &content())?;
    install_bundle(&dir.join("runbooks"), BUNDLED_RUNBOOKS)?;
    install_bundle(&dir.join("okf"), BUNDLED_OKF)?;
    let path = base.join(".grok/config.toml");
    let mut config: toml::Value = if path.exists() {
        toml::from_str(&std::fs::read_to_string(&path)?).context("parse Grok project config")?
    } else {
        toml::Value::Table(toml::map::Map::new())
    };
    let servers = config
        .as_table_mut()
        .context("Grok config must be a table")?
        .entry("mcp_servers".to_string())
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
        .as_table_mut()
        .context("Grok mcp_servers must be a table")?;
    // Preserve an explicitly configured server, including an intentional disable.
    if !servers.contains_key("agent-doc") {
        servers.insert(
            "agent-doc".into(),
            toml::from_str("command = 'agent-doc'\nargs = ['mcp']\nenabled = true\n")?,
        );
        write_if_changed(&path, &toml::to_string_pretty(&config)?)?;
    }
    eprintln!(
        "[Grok Build] installed skill v{VERSION} -> {}",
        dir.display()
    );
    Ok(())
}

pub fn check(root: Option<&Path>) -> Result<()> {
    let base = root
        .map(Path::to_path_buf)
        .unwrap_or(std::env::current_dir()?);
    if !base.join(REL_PATH).join("SKILL.md").exists() {
        anyhow::bail!("Grok skill missing; run agent-doc skill install --harness grok");
    }
    audit(&base)
}

pub(super) fn audit(base: &Path) -> Result<()> {
    let dir = base.join(REL_PATH);
    let path = dir.join("SKILL.md");
    if !path.exists() {
        return Ok(());
    }
    if std::fs::read_to_string(&path)? != content() {
        anyhow::bail!(
            "stale Grok skill {}; run agent-doc skill install --harness grok",
            path.display()
        );
    }
    for (subdir, bundle) in [("runbooks", BUNDLED_RUNBOOKS), ("okf", BUNDLED_OKF)] {
        for (name, expected) in bundle {
            if std::fs::read_to_string(dir.join(subdir).join(name))? != *expected {
                anyhow::bail!("stale Grok bundled {subdir}/{name}");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fresh_install_registers_mcp_and_keeps_unrelated_servers() {
        let dir = tempfile::tempdir().unwrap();
        assert!(check(Some(dir.path())).is_err());
        std::fs::create_dir_all(dir.path().join(".grok")).unwrap();
        let path = dir.path().join(".grok/config.toml");
        std::fs::write(&path, "[mcp_servers.other]\ncommand='other-server'\n").unwrap();
        install(Some(dir.path())).unwrap();
        let config: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            config["mcp_servers"]["other"]["command"].as_str(),
            Some("other-server")
        );
        assert_eq!(
            config["mcp_servers"]["agent-doc"]["command"].as_str(),
            Some("agent-doc")
        );
        assert_eq!(
            config["mcp_servers"]["agent-doc"]["args"][0].as_str(),
            Some("mcp")
        );
        audit(dir.path()).unwrap();
        check(Some(dir.path())).unwrap();
    }

    #[test]
    fn install_preserves_server_and_audits_shared_bundle() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".grok")).unwrap();
        let path = dir.path().join(".grok/config.toml");
        let custom = "# operator configuration\n[mcp_servers.agent-doc]\ncommand='custom-agent-doc'\nenabled=false\n";
        std::fs::write(&path, custom).unwrap();
        install(Some(dir.path())).unwrap();
        install(Some(dir.path())).unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), custom);
        audit(dir.path()).unwrap();
        let text = content();
        assert!(text.contains("agent_doc_admit"));
        assert!(text.contains("agent_doc_finalize"));
        assert!(!text.contains("the turn context must contain the"));
        assert!(!text.contains("When all of these hold, invoke the `Skill` tool"));
        std::fs::write(dir.path().join(REL_PATH).join("SKILL.md"), "stale").unwrap();
        assert!(audit(dir.path()).is_err());
    }
}
