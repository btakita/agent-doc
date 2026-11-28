use anyhow::{Context, Result, ensure};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(super) struct DraftEditor {
    command: Command,
    path: PathBuf,
    context_marker: String,
    quoted_row: String,
    _directory: tempfile::TempDir,
    #[cfg(unix)]
    _interrupt: InterruptGuard,
}

impl DraftEditor {
    pub(super) fn prepare(draft: &str, selected_row: &str) -> Result<Self> {
        let command = editor_command(std::env::var_os("VISUAL"), std::env::var_os("EDITOR"))?;
        let mut builder = tempfile::Builder::new();
        builder.prefix("agent-doc-prompt-");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            builder.permissions(fs::Permissions::from_mode(0o700));
        }
        let directory = builder
            .tempdir()
            .context("create private editor directory")?;
        let path = directory.path().join("prompt.md");
        let context_marker = format!(
            "<!-- Agent Doc: Selected row for reference. Write your prompt above or below it. {} -->",
            uuid::Uuid::new_v4().simple()
        );
        let quoted_row = format!("> {selected_row}");
        fs::write(&path, format!("{draft}\n{context_marker}\n{quoted_row}\n"))
            .context("write temporary draft")?;
        Ok(Self {
            command,
            path,
            context_marker,
            quoted_row,
            _directory: directory,
            #[cfg(unix)]
            _interrupt: InterruptGuard::install()?,
        })
    }

    pub(super) fn edit(&mut self) -> Result<String> {
        let contents = read_edited_draft(&mut self.command, &self.path)?;
        Ok(without_context(
            contents,
            &self.context_marker,
            &self.quoted_row,
        ))
    }
}

fn without_context(contents: String, marker: &str, quoted_row: &str) -> String {
    fn strip_leading_line_break(text: &str) -> &str {
        text.strip_prefix("\r\n")
            .or_else(|| text.strip_prefix('\n'))
            .unwrap_or(text)
    }

    let Some((before, rest)) = contents.split_once(marker) else {
        return contents;
    };
    let after_marker = strip_leading_line_break(rest);
    let after = after_marker
        .strip_prefix(quoted_row)
        .map_or(after_marker, strip_leading_line_break);
    // Editors can normalize line endings. Drop the template separators at
    // either edge, but keep a line break between authored text on both sides.
    let before_prompt = before
        .strip_suffix("\r\n")
        .or_else(|| before.strip_suffix('\n'))
        .unwrap_or(before);
    if before_prompt.is_empty() {
        after.to_owned()
    } else if after.is_empty() {
        before_prompt.to_owned()
    } else {
        [before, after].concat()
    }
}

fn editor_command(visual: Option<OsString>, editor: Option<OsString>) -> Result<Command> {
    let configured = [visual, editor]
        .into_iter()
        .flatten()
        .find(|value| !value.to_string_lossy().trim().is_empty());
    let fallback = if cfg!(windows) { "notepad.exe" } else { "vi" };
    let value = match &configured {
        Some(value) => value
            .to_str()
            .context("editor command must be valid UTF-8")?,
        None => fallback,
    };
    let args = shell_words::split(value).context("invalid VISUAL or EDITOR quoting")?;
    let (program, args) = args
        .split_first()
        .filter(|(program, _)| !program.is_empty())
        .context("VISUAL or EDITOR has no executable")?;
    let mut command = Command::new(program);
    command.args(args);
    Ok(command)
}

fn read_edited_draft(command: &mut Command, path: &Path) -> Result<String> {
    let status = command
        .arg(path)
        .status()
        .with_context(|| format!("start {:?}", command.get_program()))?;
    ensure!(status.success(), "editor exited with {status}");
    fs::read_to_string(path).context("read edited draft")
}

#[cfg(unix)]
struct InterruptGuard(libc::sigaction);

#[cfg(unix)]
impl InterruptGuard {
    fn install() -> Result<Self> {
        extern "C" fn ignore_interrupt(_: libc::c_int) {}

        // A cooked terminal sends Ctrl+C to the foreground process group.
        // Catch it in Explorer while the editor runs. Exec resets the caught
        // disposition in the editor, so the editor still receives interrupts.
        // SAFETY: the handler has the required signature and does no work.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = ignore_interrupt as *const () as libc::sighandler_t;
            libc::sigemptyset(&mut action.sa_mask);
            let mut previous = std::mem::MaybeUninit::uninit();
            if libc::sigaction(libc::SIGINT, &action, previous.as_mut_ptr()) != 0 {
                return Err(std::io::Error::last_os_error()).context("handle editor interrupts");
            }
            Ok(Self(previous.assume_init()))
        }
    }
}

#[cfg(unix)]
impl Drop for InterruptGuard {
    fn drop(&mut self) {
        // SAFETY: restore the disposition returned by sigaction on entry.
        if unsafe { libc::sigaction(libc::SIGINT, &self.0, std::ptr::null_mut()) } != 0 {
            let error = std::io::Error::last_os_error();
            eprintln!("[explore] could not restore interrupt handling: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONTEXT_MARKER: &str = "<!-- selected row for reference: test -->";
    const QUOTED_ROW: &str = "> - ### A **row** with an <!-- inline note -->";

    #[test]
    fn editor_context_is_removed_without_changing_the_authored_markdown() {
        let footer = format!("{CONTEXT_MARKER}\n{QUOTED_ROW}\n");
        let untouched = format!("\n{footer}");
        assert_eq!(without_context(untouched, CONTEXT_MARKER, QUOTED_ROW), "");

        let authored = "> Quoted text\n\n<!-- Authored comment -->\n  ";
        let edited = format!("{authored}\n{footer}");
        assert_eq!(
            without_context(edited, CONTEXT_MARKER, QUOTED_ROW),
            authored
        );

        let authored = "A **prompt**\r\n";
        let edited = format!("{authored}\r\n{}", footer.replace('\n', "\r\n"));
        assert_eq!(
            without_context(edited, CONTEXT_MARKER, QUOTED_ROW),
            authored
        );

        let replaced = "A **new** prompt\n> Kept quotation\n";
        assert_eq!(
            without_context(replaced.to_owned(), CONTEXT_MARKER, QUOTED_ROW),
            replaced
        );
    }

    #[test]
    fn saved_text_below_the_selected_row_stays_in_the_draft() {
        let reference = format!("\n{CONTEXT_MARKER}\n{QUOTED_ROW}\n");
        let added = "Keep **this Markdown**.\n\n> Keep this quotation, too.";
        let edited = format!("{reference}{added}");
        assert_eq!(without_context(edited, CONTEXT_MARKER, QUOTED_ROW), added);

        let edited = format!("Existing prompt.{reference}{added}").replace('\n', "\r\n");
        let expected = format!("Existing prompt.\n{added}").replace('\n', "\r\n");
        assert_eq!(
            without_context(edited, CONTEXT_MARKER, QUOTED_ROW),
            expected
        );

        let replaced_row = format!("\n{CONTEXT_MARKER}\n{added}");
        assert_eq!(
            without_context(replaced_row, CONTEXT_MARKER, QUOTED_ROW),
            added
        );
    }

    #[test]
    fn preferred_editor_and_quoted_arguments_are_resolved_without_a_shell() {
        let command = editor_command(
            Some("'/path to/editor' --flag 'one argument'".into()),
            Some("other-editor".into()),
        )
        .unwrap();
        assert_eq!(command.get_program(), "/path to/editor");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            ["--flag", "one argument"]
        );
        let command = editor_command(Some(" ".into()), Some("other-editor".into())).unwrap();
        assert_eq!(command.get_program(), "other-editor");
        assert!(editor_command(Some("editor 'unterminated".into()), None).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn draft_is_passed_via_a_file_and_failed_edit_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("draft with spaces.md");
        let previous = "literal `code` and $(command)\n";
        fs::write(&path, previous).unwrap();
        let mut command = Command::new("sh");
        command.args([
            "-c",
            "printf '\\n%s\\n' 'new draft' >> \"$1\"",
            "test-editor",
        ]);
        let edited = read_edited_draft(&mut command, &path).unwrap();
        assert_eq!(edited, format!("{previous}\nnew draft\n"));

        let mut failed = Command::new("sh");
        failed.args(["-c", "printf broken > \"$1\"; exit 1", "test-editor"]);
        let error = read_edited_draft(&mut failed, &path).unwrap_err();
        assert!(error.to_string().contains("editor exited with"));
    }
}
