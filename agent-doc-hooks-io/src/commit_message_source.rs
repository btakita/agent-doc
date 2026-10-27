//! What a `git commit` inside a shell command would actually record
//! (`#hookstalemsgfile`).
//!
//! The coined-id guard runs BEFORE the command, so a `-F <file>` message read
//! from disk is the file as it stood before this command — not the message the
//! commit will record when the command itself writes that file first:
//!
//! ```text
//! cat > msg.txt <<'EOF'
//! fix(drain): close it
//! EOF
//! git commit -F msg.txt
//! ```
//!
//! Reading `msg.txt` from disk there scans the PREVIOUS commit's message (a
//! false positive whenever that one carried an id — observed 2026-10-10, one
//! subagent blocked by an id another left in a shared scratchpad `msg.txt`),
//! and the new heredoc body is never scanned at all (a false negative, since
//! the old splitter cut segments at every newline).
//!
//! This module reads the command the way a shell would, closely enough to
//! answer that one question: quotes, heredocs, here-strings, output
//! redirections, pipes, `cd`, and `git -C`. It is not a shell. Anything it
//! cannot resolve — a path built from a variable, a file written by a program
//! whose output it cannot know, a `cd` to an unknown place — contributes
//! nothing rather than an approximation, because the guard fails open by
//! policy and a stale or guessed message is how the scan drifted from the
//! message in the first place.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

/// One shell word after quote removal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Word {
    pub(crate) value: String,
    /// False when the word carries an expansion (`$`, a backtick) whose value
    /// cannot be known from the unexpanded command.
    pub(crate) literal: bool,
}

/// One simple command of a command line.
#[derive(Debug, Default)]
pub(crate) struct Segment {
    /// Source text of the segment, excluding heredoc bodies.
    pub(crate) text: String,
    pub(crate) words: Vec<Word>,
    /// Bodies of heredocs and here-strings this segment reads on stdin.
    stdin_bodies: Vec<String>,
    /// Stdout redirections: (target, append).
    redirects: Vec<(Word, bool)>,
    /// This segment's stdout feeds the next segment (`|`).
    pipes_into_next: bool,
}

/// What the lexer expects the next completed word to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    Word,
    /// Target of a stdout (`fd == 1`) or other-fd redirection.
    Redirect {
        stdout: bool,
        append: bool,
    },
    /// Delimiter of a heredoc (`<<-` strips leading tabs).
    HeredocDelimiter {
        strip_tabs: bool,
    },
    HereString,
    /// `<` input file or `>&` duplication target: consumed, not recorded.
    Ignored,
}

struct Lexer<'a> {
    src: &'a str,
    segments: Vec<Segment>,
    current: Segment,
    segment_start: usize,
    word: String,
    word_started: bool,
    word_literal: bool,
    pending: Pending,
    /// Heredocs whose bodies start after the next unquoted newline:
    /// (segment index, delimiter, strip_tabs).
    heredocs: Vec<(usize, String, bool)>,
}

impl<'a> Lexer<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            src,
            segments: Vec::new(),
            current: Segment::default(),
            segment_start: 0,
            word: String::new(),
            word_started: false,
            word_literal: true,
            pending: Pending::Word,
            heredocs: Vec::new(),
        }
    }

    fn finish_word(&mut self) {
        if !self.word_started {
            return;
        }
        let word = Word {
            value: std::mem::take(&mut self.word),
            literal: self.word_literal,
        };
        self.word_started = false;
        self.word_literal = true;
        match std::mem::replace(&mut self.pending, Pending::Word) {
            Pending::Word => self.current.words.push(word),
            Pending::Redirect { stdout, append } => {
                if stdout {
                    self.current.redirects.push((word, append));
                }
            }
            Pending::HeredocDelimiter { strip_tabs } => {
                self.heredocs
                    .push((self.segments.len(), word.value, strip_tabs));
            }
            Pending::HereString => {
                let mut body = word.value;
                body.push('\n');
                self.current.stdin_bodies.push(body);
            }
            Pending::Ignored => {}
        }
    }

    fn finish_segment(&mut self, end: usize, pipes_into_next: bool) {
        self.finish_word();
        self.pending = Pending::Word;
        let mut segment = std::mem::take(&mut self.current);
        segment.text = self.src[self.segment_start.min(end)..end].to_string();
        segment.pipes_into_next = pipes_into_next;
        self.segments.push(segment);
    }

    fn start_word(&mut self) {
        self.word_started = true;
    }

    /// Consume the bodies of every heredoc opened on the line that just ended.
    /// `pos` is the byte offset just past the newline; returns the offset past
    /// the last body consumed.
    fn consume_heredoc_bodies(&mut self, mut pos: usize) -> usize {
        for (index, delimiter, strip_tabs) in std::mem::take(&mut self.heredocs) {
            let mut body = String::new();
            loop {
                if pos >= self.src.len() {
                    break;
                }
                let rest = &self.src[pos..];
                let (line, advance) = match rest.find('\n') {
                    Some(newline) => (&rest[..newline], newline + 1),
                    None => (rest, rest.len()),
                };
                pos += advance;
                let line = if strip_tabs {
                    line.trim_start_matches('\t')
                } else {
                    line
                };
                if line == delimiter {
                    break;
                }
                body.push_str(line);
                body.push('\n');
            }
            if let Some(segment) = self.segments.get_mut(index) {
                segment.stdin_bodies.push(body);
            }
        }
        pos
    }

    fn run(mut self) -> Vec<Segment> {
        let bytes = self.src.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let c = self.src[i..].chars().next().unwrap_or('\0');
            let len = c.len_utf8();
            match c {
                '\'' => {
                    self.start_word();
                    let close = self.src[i + 1..]
                        .find('\'')
                        .map(|offset| i + 1 + offset)
                        .unwrap_or(self.src.len());
                    self.word.push_str(&self.src[i + 1..close]);
                    i = (close + 1).min(self.src.len());
                    continue;
                }
                '"' => {
                    self.start_word();
                    let mut j = i + 1;
                    while j < bytes.len() {
                        let d = self.src[j..].chars().next().unwrap_or('\0');
                        match d {
                            '"' => break,
                            '\\' if j + 1 < bytes.len() => {
                                let next = self.src[j + 1..].chars().next().unwrap_or('\0');
                                if matches!(next, '"' | '\\' | '$' | '`') {
                                    self.word.push(next);
                                } else if next != '\n' {
                                    self.word.push('\\');
                                    self.word.push(next);
                                }
                                j += 1 + next.len_utf8();
                                continue;
                            }
                            '$' | '`' => self.word_literal = false,
                            _ => {}
                        }
                        self.word.push(d);
                        j += d.len_utf8();
                    }
                    i = (j + 1).min(self.src.len());
                    continue;
                }
                '\\' => {
                    let next = self.src[i + 1..].chars().next();
                    match next {
                        Some('\n') => {}
                        Some(next) => {
                            self.start_word();
                            self.word.push(next);
                        }
                        None => {}
                    }
                    i += 1 + next.map(char::len_utf8).unwrap_or(0);
                    continue;
                }
                '#' if !self.word_started => {
                    // A comment runs to the end of the line; the newline itself
                    // still separates commands.
                    i = self.src[i..]
                        .find('\n')
                        .map(|offset| i + offset)
                        .unwrap_or(self.src.len());
                    continue;
                }
                '\n' => {
                    self.finish_segment(i, false);
                    let after = self.consume_heredoc_bodies(i + 1);
                    self.segment_start = after;
                    i = after;
                    continue;
                }
                ';' | '&' | '|' => {
                    let next = self.src[i + 1..].chars().next();
                    // `&>` / `&>>` redirect both streams, stdout included.
                    if c == '&' && next == Some('>') {
                        self.finish_word();
                        let append = self.src[i + 2..].starts_with('>');
                        self.pending = Pending::Redirect {
                            stdout: true,
                            append,
                        };
                        i += if append { 3 } else { 2 };
                        continue;
                    }
                    let doubled = next == Some(c);
                    let pipe = c == '|' && !doubled;
                    self.finish_segment(i, pipe);
                    let mut advance = 1 + usize::from(doubled);
                    // `|&` pipes stderr too.
                    if pipe && next == Some('&') {
                        advance = 2;
                    }
                    i += advance;
                    self.segment_start = i;
                    continue;
                }
                '(' | ')' if self.word.is_empty() || c == ')' => {
                    self.finish_word();
                    i += 1;
                    continue;
                }
                '>' => {
                    let fd = if self.word_started
                        && !self.word.is_empty()
                        && self.word.bytes().all(|b| b.is_ascii_digit())
                    {
                        let fd = std::mem::take(&mut self.word);
                        self.word_started = false;
                        self.word_literal = true;
                        fd
                    } else {
                        self.finish_word();
                        "1".to_string()
                    };
                    let rest = &self.src[i + 1..];
                    let (append, dup, advance) = if rest.starts_with('>') {
                        (true, false, 2)
                    } else if rest.starts_with('&') {
                        (false, true, 2)
                    } else if rest.starts_with('|') {
                        (false, false, 2)
                    } else {
                        (false, false, 1)
                    };
                    self.pending = if dup {
                        Pending::Ignored
                    } else {
                        Pending::Redirect {
                            stdout: fd == "1",
                            append,
                        }
                    };
                    i += advance;
                    continue;
                }
                '<' => {
                    self.finish_word();
                    let rest = &self.src[i + 1..];
                    if rest.starts_with("<<") {
                        self.pending = Pending::HereString;
                        i += 3;
                    } else if rest.starts_with("<-") {
                        self.pending = Pending::HeredocDelimiter { strip_tabs: true };
                        i += 3;
                    } else if rest.starts_with('<') {
                        self.pending = Pending::HeredocDelimiter { strip_tabs: false };
                        i += 2;
                    } else {
                        self.pending = Pending::Ignored;
                        i += 1 + usize::from(rest.starts_with('&'));
                    }
                    continue;
                }
                '$' | '`' => {
                    self.start_word();
                    self.word_literal = false;
                    self.word.push(c);
                }
                '~' if !self.word_started => {
                    self.start_word();
                    let rest = &self.src[i + 1..];
                    let bare = rest.is_empty()
                        || rest.starts_with('/')
                        || rest.starts_with(char::is_whitespace);
                    match std::env::var("HOME") {
                        Ok(home) if bare => self.word.push_str(&home),
                        _ => {
                            self.word_literal = false;
                            self.word.push('~');
                        }
                    }
                }
                c if c.is_whitespace() => self.finish_word(),
                c => {
                    self.start_word();
                    self.word.push(c);
                }
            }
            i += len;
        }
        let end = self.src.len();
        if self.segment_start < end || self.word_started || !self.current.words.is_empty() {
            self.finish_segment(end, false);
        }
        self.segments
    }
}

pub(crate) fn shell_segments(command: &str) -> Vec<Segment> {
    Lexer::new(command).run()
}

/// The words of a segment with wrapper prefixes (`sudo`, `env`, `rtk proxy`)
/// and leading `NAME=value` assignments removed.
fn command_words(segment: &Segment) -> &[Word] {
    let skip = segment
        .words
        .iter()
        .take_while(|word| {
            matches!(word.value.as_str(), "sudo" | "env" | "rtk" | "proxy")
                || (word.value.contains('=') && !word.value.starts_with('-'))
        })
        .count();
    &segment.words[skip..]
}

/// For a `git ... commit` segment: the `-C` directories (in order) and the
/// words after `commit`. `None` when the segment is not a commit.
fn git_commit_parts(segment: &Segment) -> Option<(Vec<&Word>, &[Word])> {
    let words = command_words(segment);
    if words.first().map(|word| word.value.as_str()) != Some("git") {
        return None;
    }
    let mut dirs = Vec::new();
    let mut index = 1;
    while let Some(word) = words.get(index) {
        if !word.value.starts_with('-') {
            break;
        }
        // Global flags may take a VALUE (`git -C /repo commit`); consuming only
        // the flag would mistake that value for the subcommand.
        let takes_value = matches!(
            word.value.as_str(),
            "-C" | "-c" | "--git-dir" | "--work-tree" | "--namespace" | "--exec-path"
        );
        if word.value == "-C" {
            dirs.push(words.get(index + 1)?);
        }
        index += 1 + usize::from(takes_value);
    }
    (words.get(index)?.value == "commit").then(|| (dirs, &words[index + 1..]))
}

/// `-F` / `--file` values of a commit's argument list.
fn message_file_words(args: &[Word]) -> Vec<&Word> {
    let mut files = Vec::new();
    let mut index = 0;
    while let Some(word) = args.get(index) {
        if word.value == "--" {
            break;
        }
        if matches!(word.value.as_str(), "-F" | "--file") {
            if let Some(value) = args.get(index + 1) {
                files.push(value);
            }
            index += 2;
            continue;
        }
        index += 1;
    }
    files
}

/// A message file value carried inline: `--file=PATH` or `-FPATH`.
fn inline_message_file(word: &Word) -> Option<Word> {
    let value = word.value.strip_prefix("--file=").or_else(|| {
        word.value
            .strip_prefix("-F")
            .filter(|rest| !rest.is_empty())
    })?;
    Some(Word {
        value: value.to_string(),
        literal: word.literal,
    })
}

/// Lexically normalize `.` and `..` so `./msg.txt` and `msg.txt` name the same
/// write. Symlinks are not resolved; a path the lexer cannot match is simply
/// read from disk.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Resolve `word` against `dir`; `None` when either cannot be known.
fn resolve(dir: Option<&Path>, word: &Word) -> Option<PathBuf> {
    if !word.literal || word.value.is_empty() {
        return None;
    }
    let path = Path::new(&word.value);
    if path.is_absolute() {
        return Some(normalize(path));
    }
    Some(normalize(&dir?.join(path)))
}

/// What a segment's stdin carries, when knowable.
fn stdin_of(segments: &[Segment], index: usize) -> Option<String> {
    let segment = &segments[index];
    if !segment.stdin_bodies.is_empty() {
        return Some(segment.stdin_bodies.concat());
    }
    let previous = index.checked_sub(1)?;
    if segments[previous].pipes_into_next {
        return stdout_of(segments, previous);
    }
    None
}

/// What a segment writes to stdout, when knowable from the command alone:
/// `echo`/`printf` arguments, or `cat`/`tee` passing a known stdin through.
/// Any other program's output is unknown.
fn stdout_of(segments: &[Segment], index: usize) -> Option<String> {
    let words = command_words(&segments[index]);
    let (program, args) = words.split_first()?;
    match program.value.as_str() {
        "echo" | "printf" => Some(
            args.iter()
                .map(|word| word.value.as_str())
                .collect::<Vec<_>>()
                .join(" ")
                + "\n",
        ),
        "cat" if args.iter().all(|word| word.value == "-") => stdin_of(segments, index),
        "tee" => stdin_of(segments, index),
        _ => None,
    }
}

/// Everything the commit in `command` would record as its message, or `None`
/// when the command records no commit. `cwd` is the directory the command runs
/// in; `None` resolves relative paths against this process's directory.
pub(crate) fn commit_scan_text_in(command: &str, cwd: Option<&Path>) -> Option<String> {
    let segments = shell_segments(command);
    // Files this command writes before the commit runs: path -> content, where
    // `None` means "written, but with content this scan cannot know".
    let mut written: BTreeMap<PathBuf, Option<String>> = BTreeMap::new();
    let mut dir: Option<PathBuf> = Some(cwd.map(Path::to_path_buf).unwrap_or_default());

    for (index, segment) in segments.iter().enumerate() {
        if let Some((git_dirs, args)) = git_commit_parts(segment) {
            // git changes into each `-C` directory before reading `-F`, so a
            // relative message path resolves against the last one.
            let mut commit_dir = dir.clone();
            for git_dir in git_dirs {
                commit_dir = resolve(commit_dir.as_deref(), git_dir);
            }
            let mut text = segment.text.clone();
            let mut files: Vec<Word> = message_file_words(args).into_iter().cloned().collect();
            files.extend(args.iter().filter_map(inline_message_file));
            for file in files {
                let contents = if file.value == "-" {
                    stdin_of(&segments, index)
                } else {
                    match resolve(commit_dir.as_deref(), &file) {
                        Some(path) => match written.get(&path) {
                            Some(contents) => contents.clone(),
                            None => std::fs::read_to_string(&path).ok(),
                        },
                        None => None,
                    }
                };
                if let Some(contents) = contents {
                    text.push('\n');
                    text.push_str(&contents);
                }
            }
            return Some(text);
        }

        let words = command_words(segment);
        let program = words.first().map(|word| word.value.as_str());
        if matches!(program, Some("cd" | "pushd")) {
            dir = match words.get(1) {
                Some(target) if target.value != "-" => resolve(dir.as_deref(), target),
                _ => None,
            };
            continue;
        }

        let mut record = |target: Option<PathBuf>, content: Option<String>, append: bool| {
            let Some(path) = target else { return };
            let content = if append {
                let prior = match written.get(&path) {
                    Some(prior) => prior.clone(),
                    None => Some(std::fs::read_to_string(&path).unwrap_or_default()),
                };
                prior.zip(content).map(|(prior, new)| prior + &new)
            } else {
                content
            };
            written.insert(path, content);
        };
        for (target, append) in &segment.redirects {
            record(
                resolve(dir.as_deref(), target),
                stdout_of(&segments, index),
                *append,
            );
        }
        if program == Some("tee") {
            let append = words[1..]
                .iter()
                .any(|word| matches!(word.value.as_str(), "-a" | "--append"));
            for target in words[1..]
                .iter()
                .filter(|word| !word.value.starts_with('-'))
            {
                record(
                    resolve(dir.as_deref(), target),
                    stdin_of(&segments, index),
                    append,
                );
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(segment: &Segment) -> Vec<&str> {
        segment
            .words
            .iter()
            .map(|word| word.value.as_str())
            .collect()
    }

    #[test]
    fn quotes_keep_separators_inside_one_segment() {
        let segments = shell_segments("git commit -m 'a && b; c | d' && git push");
        assert_eq!(segments.len(), 2);
        assert_eq!(
            values(&segments[0]),
            ["git", "commit", "-m", "a && b; c | d"]
        );
    }

    #[test]
    fn heredoc_bodies_attach_to_their_segment_and_leave_the_text() {
        let segments =
            shell_segments("cat > msg.txt <<'EOF'\nline #one\nEOF\ngit commit -F msg.txt");
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].stdin_bodies, ["line #one\n"]);
        assert_eq!(segments[0].redirects[0].0.value, "msg.txt");
        assert!(!segments[0].text.contains("#one"));
        assert_eq!(values(&segments[1]), ["git", "commit", "-F", "msg.txt"]);
    }

    #[test]
    fn a_tab_stripped_heredoc_ends_at_an_indented_delimiter() {
        let segments = shell_segments("cat <<-EOF > m\n\tbody\n\tEOF\necho done");
        assert_eq!(segments[0].stdin_bodies, ["body\n"]);
        assert_eq!(values(&segments[1]), ["echo", "done"]);
    }

    #[test]
    fn stderr_redirects_and_duplications_are_not_stdout_writes() {
        let segments = shell_segments("printf x 2>err.log >out.txt 2>&1");
        let targets: Vec<_> = segments[0]
            .redirects
            .iter()
            .map(|(word, _)| word.value.as_str())
            .collect();
        assert_eq!(targets, ["out.txt"]);
        assert_eq!(values(&segments[0]), ["printf", "x"]);
    }

    #[test]
    fn expansions_mark_a_word_unresolvable() {
        let segments = shell_segments("git commit -F \"$DIR/msg.txt\" -F 'lit.txt'");
        assert!(!segments[0].words[3].literal);
        assert!(segments[0].words[5].literal);
    }

    #[test]
    fn git_c_and_cd_compose_into_the_message_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("repo")).unwrap();
        std::fs::write(dir.path().join("repo/msg.txt"), "from disk\n").unwrap();
        let command = format!(
            "cd {} && git -C repo commit -F ./msg.txt",
            dir.path().display()
        );
        let text = commit_scan_text_in(&command, Some(Path::new("/nonexistent"))).unwrap();
        assert!(text.contains("from disk"), "{text}");
    }

    #[test]
    fn a_cd_to_an_unknown_place_leaves_relative_message_paths_unread() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("msg.txt"), "stale\n").unwrap();
        let text =
            commit_scan_text_in("cd \"$WORK\" && git commit -F msg.txt", Some(dir.path())).unwrap();
        assert!(!text.contains("stale"), "{text}");
    }

    #[test]
    fn piped_and_teed_messages_are_followed() {
        let text =
            commit_scan_text_in("printf 'subject\\n\\nRefs body' | git commit -F -", None).unwrap();
        assert!(text.contains("Refs body"), "{text}");

        let dir = tempfile::tempdir().unwrap();
        let command = "tee m.txt <<EOF >/dev/null\nteed body\nEOF\ngit commit -F m.txt";
        let text = commit_scan_text_in(command, Some(dir.path())).unwrap();
        assert!(text.contains("teed body"), "{text}");
    }

    #[test]
    fn an_append_extends_the_file_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("m.txt"), "kept\n").unwrap();
        let text =
            commit_scan_text_in("echo added >> m.txt; git commit -F m.txt", Some(dir.path()))
                .unwrap();
        assert!(text.contains("kept") && text.contains("added"), "{text}");
    }
}
