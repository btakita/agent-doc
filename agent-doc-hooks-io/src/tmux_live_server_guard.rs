//! `#tmuxkittyterminal` mid-turn guard — block an agent's shell command that
//! would kill the operator's live tmux server, or its sessions, windows, or
//! panes.
//!
//! Observed 2026-10-26 22:26:47 -04:00: a background subagent tearing down its
//! test tmux server ran
//!
//! ```text
//! TMUX_TMPDIR=$CS/home/tmux tmux ls; TMUX_TMPDIR=$CS/home/tmux tmux kill-server
//! ```
//!
//! from a harness running inside the operator's tmux. tmux resolves its server
//! as `-L`/`-S` first, then `$TMUX`, and only then
//! `$TMUX_TMPDIR/tmux-<uid>/default`. With `$TMUX` inherited from the pane,
//! `TMUX_TMPDIR` changed nothing: `tmux ls` listed the operator's three
//! sessions and `kill-server` killed the operator's server. Every Claude and
//! Codex pane died with it, and kitty, which runs `kitty -e tmux`, closed when
//! its tmux client exited. The subagent's own `env.sh` did `unset TMUX`, but
//! the cleanup command did not source it.
//!
//! The guard is narrow by design. It inspects shell commands only (`Bash`, and
//! Grok Build's `run_terminal_command`/`Shell`, see [`SHELL_TOOL_NAMES`]), and only
//! the commands that destroy tmux state: any `kill*` command (`kill-server`,
//! `kill-session`, `kill-window`, `kill-pane`, `killp`, `killw`), and
//! `respawn*`/`unlink*` with `-k`. It also inspects `pkill`/`killall` patterns
//! that match tmux or kitty. Everything else is allowed. A destructive tmux
//! command is allowed only when it provably reaches a server other than the
//! operator's:
//!
//! * `-L <name>` (not `default`) or `-S <path>` (not the live socket); or
//! * `$TMUX` cleared (`env -u TMUX`, `TMUX=`, `unset TMUX`, `env -i`) together
//!   with a `TMUX_TMPDIR` the same command sets; or
//! * `$TMUX` pointed at a socket that is not the live one.
//!
//! A `source`d file is not followed, so `. env.sh && tmux kill-server` is
//! refused even when `env.sh` unsets `TMUX`. That costs one retry with `-L`.
//! A false allow costs every live pane. A pattern built from a shell variable
//! cannot be judged and is allowed.

use std::path::{Path, PathBuf};

use crate::coined_id_pretooluse::PreToolUseDecision;
use crate::commit_message_source::{Segment, Word, shell_segments};

/// What the hook process inherited from the harness: the inputs tmux itself
/// uses to pick a server when a command names none.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TmuxAmbient {
    /// `$TMUX` (`<socket>,<pid>,<session>`) when the harness runs inside tmux.
    pub tmux: Option<String>,
    /// `$TMUX_TMPDIR` as the harness exported it.
    pub tmux_tmpdir: Option<String>,
    pub uid: u32,
}

impl TmuxAmbient {
    pub fn from_env() -> Self {
        Self {
            tmux: std::env::var("TMUX").ok(),
            tmux_tmpdir: std::env::var("TMUX_TMPDIR").ok(),
            uid: current_uid(),
        }
    }

    /// The socket of the server this harness runs inside, from `$TMUX`.
    fn live_socket(&self) -> Option<PathBuf> {
        let tmux = self.tmux.as_deref()?;
        let path = tmux.split(',').next().unwrap_or_default();
        (!path.is_empty()).then(|| PathBuf::from(path))
    }

    /// Where `tmux -L <label>` binds when the command sets no `TMUX_TMPDIR`.
    fn ambient_label_socket(&self, label: &str) -> PathBuf {
        let base = self
            .tmux_tmpdir
            .as_deref()
            .filter(|dir| !dir.is_empty())
            .unwrap_or("/tmp");
        Path::new(base)
            .join(format!("tmux-{}", self.uid))
            .join(label)
    }

    /// Whether `path` is a socket of the operator's server: the one this
    /// harness runs inside, or the default socket.
    fn is_operator_socket(&self, path: &Path) -> bool {
        self.live_socket().as_deref() == Some(path) || self.ambient_label_socket("default") == path
    }
}

#[cfg(unix)]
fn current_uid() -> u32 {
    // SAFETY: getuid has no preconditions and cannot fail.
    unsafe { libc::getuid() }
}

#[cfg(not(unix))]
fn current_uid() -> u32 {
    0
}

/// How the command line in effect so far has set `$TMUX`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TmuxVar {
    /// Unchanged: the ambient `$TMUX`, if any.
    Inherited,
    /// Unset or empty, so tmux falls back to `TMUX_TMPDIR`.
    Cleared,
    /// Set to a value this command chose.
    Explicit(Word),
}

/// The tmux-relevant environment the command line has established.
#[derive(Debug, Clone)]
struct ShellEnv {
    tmux: TmuxVar,
    /// A non-empty `TMUX_TMPDIR` that this command exported.
    tmpdir_set: bool,
}

impl ShellEnv {
    fn new() -> Self {
        Self {
            tmux: TmuxVar::Inherited,
            tmpdir_set: false,
        }
    }

    /// Apply an exported `NAME=value` (a prefix assignment, an `env` argument,
    /// or `export`).
    fn export(&mut self, name: &str, value: &Word) {
        match name {
            "TMUX" if value.value.is_empty() => self.tmux = TmuxVar::Cleared,
            "TMUX" => self.tmux = TmuxVar::Explicit(value.clone()),
            "TMUX_TMPDIR" => self.tmpdir_set = !value.value.is_empty(),
            _ => {}
        }
    }

    fn unset(&mut self, name: &str) {
        match name {
            "TMUX" => self.tmux = TmuxVar::Cleared,
            "TMUX_TMPDIR" => self.tmpdir_set = false,
            _ => {}
        }
    }
}

/// `NAME=value` as (name, value), when `word` is an assignment.
fn assignment(word: &Word) -> Option<(&str, Word)> {
    let (name, value) = word.value.split_once('=')?;
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        && !name.starts_with(|ch: char| ch.is_ascii_digit());
    valid.then(|| {
        (
            name,
            Word {
                value: value.to_string(),
                literal: word.literal,
            },
        )
    })
}

/// Process names a `pkill`/`killall` pattern must not match: the operator's
/// tmux server and client, and the terminal hosting them.
const LIVE_PROCESS_NAMES: &[&str] = &[
    "tmux",
    "tmux: server",
    "tmux: client",
    "/usr/bin/tmux",
    "kitty",
    "kitten",
    "kitty -e tmux",
    "/usr/bin/kitty",
];

/// How deep `sh -c '...'` nesting is followed.
const MAX_SHELL_DEPTH: usize = 4;

/// Tool names a harness reports for its shell tool in a `PreToolUse` payload.
///
/// Claude Code and Codex report `Bash`. Grok Build loads the same Claude
/// settings files (`~/.claude/settings.json` always, a project's
/// `.claude/settings.json` once the folder is trusted) but reports its own tool
/// name, `run_terminal_command`, or `Shell` under its Cursor preset; it maps
/// Claude names only in a `matcher`, never in the payload. Matching `Bash`
/// alone left the guard loaded and inert in every Grok pane
/// (`#tmuxguardclaudeuser`).
pub const SHELL_TOOL_NAMES: &[&str] = &["Bash", "run_terminal_command", "Shell"];

/// Whether `tool_name` is a harness shell tool this guard inspects.
pub fn is_shell_tool(tool_name: &str) -> bool {
    SHELL_TOOL_NAMES.contains(&tool_name)
}

/// Decide whether a tool call may run, given the harness environment.
pub fn tmux_live_server_decision(
    tool_name: &str,
    tool_input: &serde_json::Value,
    ambient: &TmuxAmbient,
) -> PreToolUseDecision {
    if !is_shell_tool(tool_name) {
        return PreToolUseDecision::Allow;
    }
    let command = match tool_input.get("command") {
        Some(serde_json::Value::String(command)) => command.clone(),
        Some(serde_json::Value::Array(words)) => words
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(shell_quote)
            .collect::<Vec<_>>()
            .join(" "),
        _ => return PreToolUseDecision::Allow,
    };
    match scan(&command, ShellEnv::new(), ambient, 0) {
        Some(reason) => PreToolUseDecision::Deny { reason },
        None => PreToolUseDecision::Allow,
    }
}

fn shell_quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

/// The deny reason for the first segment that would hit the operator's
/// server, or `None`.
fn scan(command: &str, mut env: ShellEnv, ambient: &TmuxAmbient, depth: usize) -> Option<String> {
    for segment in shell_segments(command) {
        if let Some(reason) = scan_segment(&segment, &mut env, ambient, depth) {
            return Some(reason);
        }
    }
    None
}

fn scan_segment(
    segment: &Segment,
    env: &mut ShellEnv,
    ambient: &TmuxAmbient,
    depth: usize,
) -> Option<String> {
    let words = &segment.words;
    let first = words.first()?;
    match first.value.as_str() {
        "unset" => {
            for word in &words[1..] {
                env.unset(&word.value);
            }
            return None;
        }
        "export" | "declare" | "typeset" => {
            for word in &words[1..] {
                if let Some((name, value)) = assignment(word) {
                    env.export(name, &value);
                }
            }
            return None;
        }
        _ => {}
    }
    // A segment of bare assignments sets shell variables. Later commands see
    // one only when it was already exported: the harness inherited it.
    if words.iter().all(|word| assignment(word).is_some()) {
        for (name, value) in words.iter().filter_map(assignment) {
            let exported = match name {
                "TMUX" => ambient.tmux.is_some(),
                "TMUX_TMPDIR" => ambient.tmux_tmpdir.is_some(),
                _ => false,
            };
            if exported {
                env.export(name, &value);
            }
        }
        return None;
    }

    // Prefix assignments and wrappers apply to this command only.
    let mut local = env.clone();
    let mut index = 0;
    while let Some(word) = words.get(index) {
        if let Some((name, value)) = assignment(word) {
            local.export(name, &value);
            index += 1;
            continue;
        }
        match word.value.as_str() {
            "env" => {
                index += 1;
                while let Some(arg) = words.get(index) {
                    let arg_value = arg.value.as_str();
                    if arg_value == "-i" || arg_value == "-" || arg_value == "--ignore-environment"
                    {
                        local.tmux = TmuxVar::Cleared;
                        local.tmpdir_set = false;
                    } else if arg_value == "-u" || arg_value == "--unset" {
                        if let Some(name) = words.get(index + 1) {
                            local.unset(&name.value);
                        }
                        index += 1;
                    } else if let Some(name) = arg_value
                        .strip_prefix("--unset=")
                        .or_else(|| arg_value.strip_prefix("-u"))
                    {
                        local.unset(name);
                    } else if arg_value == "-C" || arg_value == "--chdir" {
                        index += 1;
                    } else if let Some((name, value)) = assignment(arg) {
                        local.export(name, &value);
                    } else if !arg_value.starts_with('-') {
                        // The command `env` runs; other flags are skipped.
                        break;
                    }
                    index += 1;
                }
            }
            "sudo" | "command" | "exec" | "nohup" | "setsid" | "time" | "builtin" | "proxy"
            | "rtk" => index += 1,
            "nice" => {
                index += 1;
                if words.get(index).is_some_and(|w| w.value == "-n") {
                    index += 2;
                } else if words.get(index).is_some_and(|w| w.value.starts_with('-')) {
                    index += 1;
                }
            }
            "timeout" => {
                index += 1;
                while let Some(arg) = words.get(index) {
                    if arg.value == "-s" || arg.value == "-k" {
                        index += 2;
                    } else if arg.value.starts_with('-') {
                        index += 1;
                    } else {
                        break;
                    }
                }
                // The duration.
                index += 1;
            }
            _ => break,
        }
    }
    let command = words.get(index)?;
    let args = &words[index + 1..];
    let name = Path::new(&command.value)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    match name {
        "tmux" => tmux_command(segment, args, &local, ambient),
        "pkill" | "killall" => kill_by_name(segment, name, args),
        "sh" | "bash" | "zsh" | "dash" if depth < MAX_SHELL_DEPTH => {
            let script = shell_c_script(args)?;
            if !script.literal {
                return None;
            }
            scan(&script.value, local, ambient, depth + 1)
        }
        _ => None,
    }
}

/// The script of `sh -c <script>` (also `-lc`, `-ec`, ...).
fn shell_c_script(args: &[Word]) -> Option<&Word> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let flags = arg.value.strip_prefix('-')?;
        if flags.starts_with('-') {
            continue;
        }
        if flags.contains('c') {
            return iter.next();
        }
    }
    None
}

#[derive(Debug)]
enum SocketFlag<'a> {
    Label(&'a str, bool),
    Path(&'a str, bool),
}

fn tmux_command(
    segment: &Segment,
    args: &[Word],
    env: &ShellEnv,
    ambient: &TmuxAmbient,
) -> Option<String> {
    // Global options, getopt style: `-L name`, `-Lname`, `-uL name`.
    let mut socket = None;
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        let value = arg.value.as_str();
        if value == "--" {
            index += 1;
            break;
        }
        let Some(flags) = value.strip_prefix('-') else {
            break;
        };
        if flags.is_empty() {
            break;
        }
        for (offset, flag) in flags.char_indices() {
            if !matches!(flag, 'c' | 'f' | 'L' | 'S' | 'T') {
                continue;
            }
            let rest = &flags[offset + flag.len_utf8()..];
            let (option, literal) = if rest.is_empty() {
                index += 1;
                match args.get(index) {
                    Some(word) => (word.value.as_str(), word.literal),
                    None => ("", true),
                }
            } else {
                (rest, arg.literal)
            };
            match flag {
                'L' => socket = Some(SocketFlag::Label(option, literal)),
                'S' => socket = Some(SocketFlag::Path(option, literal)),
                _ => {}
            }
            break;
        }
        index += 1;
    }

    let destructive = destructive_commands(&args[index.min(args.len())..]);
    if destructive.is_empty() {
        return None;
    }
    let target = match socket {
        Some(SocketFlag::Label(_, false) | SocketFlag::Path(_, false)) => return None,
        Some(SocketFlag::Path(path, true)) => {
            if !ambient.is_operator_socket(Path::new(path)) {
                return None;
            }
            format!("`-S {path}`, which is the operator's server socket")
        }
        Some(SocketFlag::Label(label, true)) => {
            // `-L` beats `$TMUX`; the label resolves under this command's own
            // `TMUX_TMPDIR` when it sets one.
            let path = ambient.ambient_label_socket(label);
            if env.tmpdir_set || !ambient.is_operator_socket(&path) {
                return None;
            }
            format!(
                "`-L {label}`, which is the operator's server socket {}",
                path.display()
            )
        }
        None => match &env.tmux {
            TmuxVar::Explicit(word) => {
                let path = word.value.split(',').next().unwrap_or_default();
                if !word.literal || !ambient.is_operator_socket(Path::new(path)) {
                    return None;
                }
                format!("`$TMUX` set to the operator's server socket {path}")
            }
            TmuxVar::Cleared if env.tmpdir_set => return None,
            TmuxVar::Cleared => format!(
                "the default server {} (`$TMUX` is cleared but no `TMUX_TMPDIR` is set)",
                ambient.ambient_label_socket("default").display()
            ),
            TmuxVar::Inherited => match ambient.live_socket() {
                Some(live) if env.tmpdir_set => format!(
                    "the server this harness runs inside, {}: `$TMUX` is set, and tmux \
                     consults `$TMUX` before `TMUX_TMPDIR`, so the `TMUX_TMPDIR` in this \
                     command is ignored",
                    live.display()
                ),
                Some(live) => format!(
                    "the server this harness runs inside, {} (from `$TMUX`)",
                    live.display()
                ),
                None if env.tmpdir_set => return None,
                None => format!(
                    "the default server {}",
                    ambient.ambient_label_socket("default").display()
                ),
            },
        },
    };
    Some(format!(
        "[agent-doc] blocked: `{}` runs tmux {} with no socket of its own, so it would reach \
         {target}. That is the operator's live tmux server, where every agent pane and the \
         terminal hosting them run. On 2026-10-26 this exact shape killed the operator's \
         server, all of its panes, and kitty. Run every tmux command against a test server with \
         both `env -u TMUX` and a private socket: `env -u TMUX tmux -L <unique-name> {}` (or \
         `-S <socket-path>`). `TMUX_TMPDIR` alone isolates nothing while `$TMUX` is set. A \
         sourced env file does not count, because \
         the guard cannot see it. If the operator's own server really must change, ask the \
         operator to do it.",
        segment.text.trim(),
        destructive.join(", "),
        destructive[0],
    ))
}

/// Names of the tmux commands in `args` (split on `;`) that destroy a
/// server, session, window, or pane, or kill the process in one.
fn destructive_commands(args: &[Word]) -> Vec<String> {
    let mut found = Vec::new();
    let mut command: Vec<&str> = Vec::new();
    for arg in args {
        let value = arg.value.as_str();
        match value.strip_suffix(';') {
            Some(stripped) => {
                if !stripped.is_empty() {
                    command.push(stripped);
                }
                if let Some(name) = destructive_command(&command) {
                    found.push(name.to_string());
                }
                command.clear();
            }
            None => command.push(value),
        }
    }
    if let Some(name) = destructive_command(&command) {
        found.push(name.to_string());
    }
    found
}

/// The command's name when it destroys tmux state: any `kill*` (tmux accepts
/// a unique prefix and the `killp`/`killw` aliases), or `respawn*`/`unlink*`
/// with `-k`.
fn destructive_command<'a>(command: &[&'a str]) -> Option<&'a str> {
    let (name, rest) = command.split_first()?;
    let with_k = || {
        rest.iter()
            .any(|arg| arg.starts_with('-') && !arg.starts_with("--") && arg.contains('k'))
    };
    let destroys = name.starts_with("kill")
        || ((name.starts_with("respawn") || name.starts_with("unlink")) && with_k());
    destroys.then_some(*name)
}

/// `pkill`/`killall` whose pattern matches the operator's tmux or terminal.
fn kill_by_name(segment: &Segment, name: &str, args: &[Word]) -> Option<String> {
    let value_flags: &[&str] = if name == "pkill" {
        &[
            "-g",
            "-G",
            "-P",
            "-s",
            "-t",
            "-u",
            "-U",
            "-F",
            "--signal",
            "--pidfile",
            "--ns",
            "--nslist",
        ]
    } else {
        &[
            "-s",
            "--signal",
            "-u",
            "--user",
            "-o",
            "--older-than",
            "-y",
            "--younger-than",
            "-n",
            "--ns",
            "-Z",
            "--context",
        ]
    };
    // `killall` matches whole names unless `-r`; `pkill` matches anywhere
    // unless `-x`.
    let mut exact = name == "killall";
    let mut patterns = Vec::new();
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        let value = arg.value.as_str();
        if value_flags.contains(&value) {
            index += 2;
            continue;
        }
        let short_flags = value
            .strip_prefix('-')
            .filter(|flags| flags.chars().all(|ch| ch.is_ascii_lowercase()));
        match name {
            "pkill" if value == "--exact" || short_flags.is_some_and(|f| f.contains('x')) => {
                exact = true;
            }
            "killall" if value == "--regexp" || short_flags.is_some_and(|f| f.contains('r')) => {
                exact = false;
            }
            _ => {}
        }
        if !value.starts_with('-') {
            patterns.push(arg);
        }
        index += 1;
    }
    let hit = patterns
        .into_iter()
        .filter(|word| word.literal)
        .find(|word| {
            let pattern = word.value.as_str();
            let anchored = if exact {
                format!("^(?:{pattern})$")
            } else {
                pattern.to_string()
            };
            match regex::Regex::new(&anchored) {
                Ok(re) => LIVE_PROCESS_NAMES.iter().any(|name| re.is_match(name)),
                Err(_) => {
                    let lower = pattern.to_ascii_lowercase();
                    lower.contains("tmux") || lower.contains("kitty")
                }
            }
        })?;
    Some(format!(
        "[agent-doc] blocked: `{}` matches the operator's tmux server or terminal (pattern \
         `{}`), so it would kill every agent pane and the terminal hosting them. Stop a private \
         tmux server with `tmux -L <its-name> kill-server`, or kill a process you started by \
         its pid.",
        segment.text.trim(),
        hit.value,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn inside_tmux() -> TmuxAmbient {
        TmuxAmbient {
            tmux: Some("/tmp/tmux-1000/default,2477371,0".to_string()),
            tmux_tmpdir: None,
            uid: 1000,
        }
    }

    fn outside_tmux() -> TmuxAmbient {
        TmuxAmbient {
            tmux: None,
            tmux_tmpdir: None,
            uid: 1000,
        }
    }

    fn decide(command: &str, ambient: &TmuxAmbient) -> PreToolUseDecision {
        tmux_live_server_decision("Bash", &json!({ "command": command }), ambient)
    }

    fn denied(command: &str, ambient: &TmuxAmbient) -> String {
        match decide(command, ambient) {
            PreToolUseDecision::Deny { reason } => reason,
            PreToolUseDecision::Allow => panic!("expected deny: {command}"),
        }
    }

    fn allowed(command: &str, ambient: &TmuxAmbient) {
        assert_eq!(
            decide(command, ambient),
            PreToolUseDecision::Allow,
            "expected allow: {command}"
        );
    }

    /// The command that killed the operator's server on 2026-10-26.
    #[test]
    fn the_incident_command_is_refused_and_names_the_tmux_precedence() {
        let command = "CS=/home/brian/work/btakita/agent-doc-wt/vscoderemote/target/vscoderemote-cs; \
             TMUX_TMPDIR=$CS/home/tmux tmux ls 2>&1 | head -3; \
             TMUX_TMPDIR=$CS/home/tmux tmux kill-server 2>&1 | head -1; \
             kill -TERM 2308051 2453272 2>&1; sleep 2";
        let reason = denied(command, &inside_tmux());
        assert!(reason.contains("kill-server"), "{reason}");
        assert!(
            reason.contains("TMUX_TMPDIR` in this command is ignored"),
            "{reason}"
        );
        assert!(reason.contains("/tmp/tmux-1000/default"), "{reason}");
        assert!(
            reason.contains("env -u TMUX tmux -L <unique-name> kill-server"),
            "{reason}"
        );
    }

    #[test]
    fn every_kill_command_on_the_inherited_server_is_refused() {
        for command in [
            "tmux kill-server",
            "tmux kill-session -t 0",
            "tmux kill-window -t @3",
            "tmux kill-pane -t %2",
            "tmux killp -t %2",
            "tmux killw",
            "tmux kill-ser",
            "/usr/bin/tmux kill-server",
            "command tmux kill-server",
            "rtk proxy tmux kill-server",
            "timeout -s KILL 5 tmux kill-server",
            "nice -n 19 tmux kill-session -t main",
            "tmux respawn-pane -k -t %1",
            "tmux respawnp -kt %1",
            "tmux unlink-window -k -t @2",
            "tmux new-session -d -s scratch \\; kill-server",
            "tmux -u -f /dev/null kill-server",
            "cd /tmp && tmux kill-server",
            "sh -c 'tmux kill-server'",
            "bash -lc \"tmux kill-session -t 0\"",
        ] {
            denied(command, &inside_tmux());
        }
    }

    #[test]
    fn non_destructive_and_read_only_tmux_commands_are_allowed() {
        for command in [
            "tmux ls",
            "tmux list-panes -a -F '#{pane_id}'",
            "tmux show-options -g",
            "tmux capture-pane -p -t %2",
            "tmux respawn-pane -t %1",
            "tmux display-message -p '#{socket_path}'",
            "echo tmux kill-server",
            "grep -rn 'tmux kill-server' src",
            "git commit -m 'never run tmux kill-server'",
        ] {
            allowed(command, &inside_tmux());
        }
    }

    #[test]
    fn a_private_socket_flag_isolates_the_command() {
        for command in [
            "tmux -L lzkitty-1234 kill-server",
            "tmux -Llzkitty-1234 kill-server",
            "tmux -uL lzkitty-1234 kill-server",
            "tmux -S /home/me/wt/.tmux/sock kill-server",
            "tmux -S \"$SOCK\" kill-server",
            "tmux -L \"$NAME\" kill-session -t main",
            "TMUX_TMPDIR=/wt/tmux tmux -L default kill-server",
        ] {
            allowed(command, &inside_tmux());
        }
    }

    #[test]
    fn a_socket_flag_naming_the_operator_server_is_refused() {
        denied("tmux -L default kill-server", &inside_tmux());
        denied("tmux -S /tmp/tmux-1000/default kill-server", &inside_tmux());
        denied("tmux -L default kill-server", &outside_tmux());
        let custom = TmuxAmbient {
            tmux: Some("/run/user/1000/tmux-1000/work,1,0".to_string()),
            tmux_tmpdir: Some("/run/user/1000".to_string()),
            uid: 1000,
        };
        denied("tmux -L work kill-session -t 0", &custom);
        denied("tmux -S /run/user/1000/tmux-1000/work kill-server", &custom);
        allowed("tmux -L lzkitty-9 kill-server", &custom);
    }

    #[test]
    fn clearing_tmux_and_setting_a_private_tmpdir_isolates_the_command() {
        for command in [
            "env -u TMUX TMUX_TMPDIR=/wt/tmux tmux kill-server",
            "env -uTMUX TMUX_TMPDIR=/wt/tmux tmux kill-server",
            "env --unset=TMUX TMUX_TMPDIR=$WT/tmux tmux kill-server",
            "TMUX= TMUX_TMPDIR=/wt/tmux tmux kill-server",
            "unset TMUX; export TMUX_TMPDIR=/wt/tmux; tmux kill-server",
            "export TMUX= TMUX_TMPDIR=/wt/tmux && tmux kill-server",
            "env -i PATH=/usr/bin TMUX_TMPDIR=/wt/tmux tmux kill-server",
            "TMUX=/wt/tmux/sock,1,0 tmux kill-server",
            "unset TMUX; sh -c 'TMUX_TMPDIR=/wt/tmux tmux kill-server'",
        ] {
            allowed(command, &inside_tmux());
        }
    }

    #[test]
    fn clearing_only_one_of_the_two_still_reaches_the_operator() {
        // TMUX_TMPDIR without clearing TMUX: tmux uses $TMUX.
        denied("TMUX_TMPDIR=/wt/tmux tmux kill-server", &inside_tmux());
        denied(
            "export TMUX_TMPDIR=/wt/tmux; tmux kill-server",
            &inside_tmux(),
        );
        // TMUX cleared without TMUX_TMPDIR: tmux uses the default socket.
        denied("env -u TMUX tmux kill-server", &inside_tmux());
        denied("unset TMUX && tmux kill-session -t 0", &inside_tmux());
        // An unexported TMUX_TMPDIR never reaches tmux.
        denied(
            "TMUX_TMPDIR=/wt/tmux; unset TMUX; tmux kill-server",
            &inside_tmux(),
        );
        // A prefix assignment applies to its own command only.
        denied(
            "env -u TMUX TMUX_TMPDIR=/wt/tmux true; tmux kill-server",
            &inside_tmux(),
        );
        // TMUX pointed back at the live socket.
        denied(
            "TMUX=/tmp/tmux-1000/default,1,0 tmux kill-server",
            &inside_tmux(),
        );
        // Outside tmux a bare `TMUX=` assignment is never exported, so tmux
        // still falls back to the default server.
        denied("TMUX=/wt/tmux/sock,1,0; tmux kill-server", &outside_tmux());
        allowed("TMUX=/wt/tmux/sock,1,0; tmux kill-server", &inside_tmux());
    }

    #[test]
    fn a_sourced_env_file_is_not_trusted() {
        let reason = denied(". /wt/target/cs/env.sh && tmux kill-server", &inside_tmux());
        assert!(
            reason.contains("sourced env file does not count"),
            "{reason}"
        );
        denied("source env.sh; tmux kill-server", &inside_tmux());
    }

    #[test]
    fn outside_tmux_a_tmpdir_alone_isolates_but_the_default_server_does_not() {
        allowed("TMUX_TMPDIR=/wt/tmux tmux kill-server", &outside_tmux());
        let reason = denied("tmux kill-server", &outside_tmux());
        assert!(
            reason.contains("the default server /tmp/tmux-1000/default"),
            "{reason}"
        );
    }

    #[test]
    fn pkill_and_killall_patterns_matching_tmux_or_kitty_are_refused() {
        for command in [
            "pkill tmux",
            "pkill -f tmux",
            "pkill -9 -f '^tmux'",
            "pkill -x 'tmux: server'",
            "killall tmux",
            "killall -9 kitty",
            "pkill -f 'kitty|foo'",
            "pkill -u brian tmux",
        ] {
            denied(command, &inside_tmux());
        }
        for command in [
            "pkill -f 'vscoderemote-cs/code-server'",
            "pkill -f 'tmux -L lzkitty-1'",
            "pkill -x tmuxinator",
            "pkill -f \"$PATTERN\"",
            "killall code-server",
        ] {
            allowed(command, &inside_tmux());
        }
    }

    #[test]
    fn only_shell_tools_are_inspected() {
        let ambient = inside_tmux();
        assert_eq!(
            tmux_live_server_decision(
                "Write",
                &json!({ "file_path": "x.sh", "content": "tmux kill-server" }),
                &ambient,
            ),
            PreToolUseDecision::Allow
        );
        assert!(matches!(
            tmux_live_server_decision(
                "Bash",
                &json!({ "command": ["bash", "-lc", "tmux kill-server"] }),
                &ambient,
            ),
            PreToolUseDecision::Deny { .. }
        ));
    }

    /// `#tmuxguardclaudeuser`: Grok Build runs the hooks in Claude settings
    /// files but names its shell tool `run_terminal_command` (or `Shell` under
    /// its Cursor preset). The guard must judge those exactly like `Bash`.
    #[test]
    fn grok_shell_tool_names_are_inspected_like_bash() {
        let ambient = inside_tmux();
        for tool in ["run_terminal_command", "Shell"] {
            assert!(
                matches!(
                    tmux_live_server_decision(
                        tool,
                        &json!({ "command": "tmux kill-server" }),
                        &ambient,
                    ),
                    PreToolUseDecision::Deny { .. }
                ),
                "{tool} must be refused like Bash"
            );
            assert_eq!(
                tmux_live_server_decision(
                    tool,
                    &json!({ "command": "env -u TMUX tmux -L probe kill-server" }),
                    &ambient,
                ),
                PreToolUseDecision::Allow,
                "{tool} with a private socket must be allowed like Bash"
            );
        }
        assert_eq!(
            tmux_live_server_decision(
                "search_replace",
                &json!({ "command": "tmux kill-server" }),
                &ambient,
            ),
            PreToolUseDecision::Allow
        );
    }

    #[test]
    fn ambient_reads_the_live_socket_from_tmux() {
        let ambient = inside_tmux();
        assert_eq!(
            ambient.live_socket().as_deref(),
            Some(Path::new("/tmp/tmux-1000/default"))
        );
        assert_eq!(outside_tmux().live_socket(), None);
        let empty = TmuxAmbient {
            tmux: Some(String::new()),
            ..outside_tmux()
        };
        assert_eq!(empty.live_socket(), None);
    }
}
