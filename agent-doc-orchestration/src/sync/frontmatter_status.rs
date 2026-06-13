use super::*;

pub(crate) fn parse_frontmatter_for_sync<'a>(
    content: &'a str,
    file: &Path,
    phase: &str,
) -> Result<(frontmatter::Frontmatter, &'a str)> {
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    frontmatter::parse_for_file_with_context(content, file, &rc)
        .map_err(|err| anyhow::anyhow!("sync {} frontmatter: {}", phase, err))
}

pub(crate) fn sync_frontmatter_status_message(phase: &str, err: &anyhow::Error) -> String {
    format!(
        "{} during {}.\n\n{}",
        SYNC_FRONTMATTER_STATUS_PREFIX, phase, err
    )
}

pub(crate) fn write_sync_status(file: &Path, text: &str) -> Result<bool> {
    let doc = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {} for sync status update", file.display()))?;
    let components = component::parse(&doc)
        .with_context(|| format!("failed to parse components in {}", file.display()))?;
    let Some(status) = components
        .iter()
        .find(|comp| comp.name.as_str() == "status")
        .cloned()
    else {
        return Ok(false);
    };
    if status.content(&doc).trim() == text.trim() {
        return Ok(false);
    }

    let payload = if text.is_empty() {
        String::new()
    } else {
        format!("{text}\n")
    };
    let updated = status.replace_content(&doc, &payload);
    std::fs::write(file, &updated)
        .with_context(|| format!("failed to write {} for sync status update", file.display()))?;
    snapshot::save(file, &updated).with_context(|| {
        format!(
            "failed to update snapshot for {} after sync status update",
            file.display()
        )
    })?;
    Ok(true)
}

pub(crate) fn surface_frontmatter_status(file: &Path, phase: &str, err: &anyhow::Error) {
    let text = sync_frontmatter_status_message(phase, err);
    match write_sync_status(file, &text) {
        Ok(true) => {
            let log = format!(
                "[sync] status: surfaced malformed frontmatter warning for {}",
                file.display()
            );
            eprintln!("{}", log);
            sync_log(&log);
        }
        Ok(false) => {}
        Err(status_err) => {
            let warning = format!(
                "[sync] warning: failed to surface malformed frontmatter status for {}: {}",
                file.display(),
                status_err
            );
            eprintln!("{}", warning);
            sync_log(&warning);
        }
    }
}

pub(crate) fn clear_frontmatter_status(file: &Path) {
    let doc = match std::fs::read_to_string(file) {
        Ok(doc) => doc,
        Err(_) => return,
    };
    let components = match component::parse(&doc) {
        Ok(components) => components,
        Err(_) => return,
    };
    let Some(status) = components
        .iter()
        .find(|comp| comp.name.as_str() == "status")
        .cloned()
    else {
        return;
    };
    if !status
        .content(&doc)
        .trim_start()
        .starts_with(SYNC_FRONTMATTER_STATUS_PREFIX)
    {
        return;
    }

    match write_sync_status(file, "") {
        Ok(true) => {
            let log = format!(
                "[sync] status: cleared malformed frontmatter warning for {}",
                file.display()
            );
            eprintln!("{}", log);
            sync_log(&log);
        }
        Ok(false) => {}
        Err(status_err) => {
            let warning = format!(
                "[sync] warning: failed to clear malformed frontmatter status for {}: {}",
                file.display(),
                status_err
            );
            eprintln!("{}", warning);
            sync_log(&warning);
        }
    }
}
