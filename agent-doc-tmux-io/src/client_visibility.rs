//! Which tmux sessions an operator can actually see (GH #243).
//!
//! A tmux session with no attached client is invisible: anything agent-doc
//! starts or submits there runs on no screen. The ambient "current session"
//! tmux reports to a process that is not itself inside a pane (the project
//! controller, an IDE-launched helper) is just tmux's best guess, the most
//! recently used session, and it can name a session nobody is attached to.
//! After a workspace restart that guess picked a client-less session `0`, so
//! route-owned panes started there and every Submit answered off-screen.
//!
//! This module observes attached clients and decides, purely, which session an
//! ambient lookup should resolve to and whether a pane is on any screen.

use std::path::Path;

/// One tmux client attached to the server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachedTmuxClient {
    /// The session the client is showing.
    pub session: String,
    /// `#{client_activity}`: the client's last-input time (unix seconds).
    pub activity: u64,
    /// The session is an agent-doc installation navigation lane
    /// (`@agent_doc_navigation_lane`). A lane belongs to one IDE installation,
    /// so an ambient lookup never switches *into* someone else's lane.
    pub navigation_lane: bool,
}

/// How an ambient session lookup was resolved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AmbientSessionResolution {
    /// The caller runs inside a tmux pane; that pane's session.
    CallerPane,
    /// tmux's ambient session, and a client is attached to it.
    AmbientAttached,
    /// tmux's ambient session had no attached client, so the session of the
    /// most recently active attached client was chosen instead.
    AttachedClientPreferred,
    /// No client is attached to any eligible session; the ambient session is
    /// kept, and it is not on any screen.
    Unattached,
}

impl AmbientSessionResolution {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CallerPane => "caller_pane",
            Self::AmbientAttached => "ambient_attached",
            Self::AttachedClientPreferred => "attached_client_preferred",
            Self::Unattached => "unattached",
        }
    }
}

/// The resolved ambient session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AmbientSession {
    pub session: String,
    pub resolution: AmbientSessionResolution,
    /// The client-less ambient session that was passed over, if any.
    pub passed_over: Option<String>,
}

/// Pure choice of the ambient session (GH #243).
///
/// Keeps tmux's ambient session when a client shows it. Otherwise prefers the
/// session of the most recently active attached client (never an agent-doc
/// navigation lane, which belongs to one installation). With no eligible
/// attached client the ambient session is kept and reported as `Unattached`.
pub fn choose_ambient_session(
    ambient: Option<&str>,
    attached: &[AttachedTmuxClient],
) -> Option<AmbientSession> {
    let ambient = ambient.map(str::trim).filter(|session| !session.is_empty());
    if let Some(ambient) = ambient
        && attached.iter().any(|client| client.session == ambient)
    {
        return Some(AmbientSession {
            session: ambient.to_string(),
            resolution: AmbientSessionResolution::AmbientAttached,
            passed_over: None,
        });
    }
    let preferred = attached
        .iter()
        .filter(|client| !client.navigation_lane && !client.session.trim().is_empty())
        .max_by(|left, right| {
            left.activity
                .cmp(&right.activity)
                // Deterministic tie-break: the lexically smaller session wins.
                .then_with(|| right.session.cmp(&left.session))
        });
    match (preferred, ambient) {
        (Some(client), ambient) => Some(AmbientSession {
            session: client.session.clone(),
            resolution: AmbientSessionResolution::AttachedClientPreferred,
            passed_over: ambient.map(str::to_string),
        }),
        (None, Some(ambient)) => Some(AmbientSession {
            session: ambient.to_string(),
            resolution: AmbientSessionResolution::Unattached,
            passed_over: None,
        }),
        (None, None) => None,
    }
}

/// Parse `list-clients -F '#{client_activity}\t#{session_name}\t#{@agent_doc_navigation_lane}'`.
pub fn parse_attached_clients(output: &str) -> Vec<AttachedTmuxClient> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.splitn(3, '\t');
            let activity = fields.next()?.trim().parse::<u64>().unwrap_or(0);
            let session = fields.next()?.trim();
            if session.is_empty() {
                return None;
            }
            let navigation_lane = fields.next().is_some_and(|lane| !lane.trim().is_empty());
            Some(AttachedTmuxClient {
                session: session.to_string(),
                activity,
                navigation_lane,
            })
        })
        .collect()
}

/// Every client attached to `tmux`'s server. A server that is not running, or
/// a failed query, has no attached clients.
pub fn attached_clients(tmux: &tmux_router::Tmux) -> Vec<AttachedTmuxClient> {
    let Ok(output) = tmux
        .cmd()
        .args([
            "list-clients",
            "-F",
            "#{client_activity}\t#{session_name}\t#{@agent_doc_navigation_lane}",
        ])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    parse_attached_clients(&String::from_utf8_lossy(&output.stdout))
}

/// Number of clients attached to `session`.
pub fn session_attached_client_count(tmux: &tmux_router::Tmux, session: &str) -> usize {
    attached_clients(tmux)
        .iter()
        .filter(|client| client.session == session)
        .count()
}

/// Resolve the session a process means by "current": the session of
/// `current_pane` when the caller runs inside a live pane, otherwise tmux's
/// ambient session, preferring a session a client is attached to
/// ([`choose_ambient_session`]).
pub fn ambient_current_session(
    tmux: &tmux_router::Tmux,
    current_pane: Option<&str>,
) -> Option<AmbientSession> {
    if let Some(pane) = current_pane.map(str::trim).filter(|pane| !pane.is_empty())
        && let Ok(session) = tmux.pane_session(pane)
        && !session.is_empty()
    {
        return Some(AmbientSession {
            session,
            resolution: AmbientSessionResolution::CallerPane,
            passed_over: None,
        });
    }
    let ambient = tmux.current_session_for_pane(None);
    choose_ambient_session(ambient.as_deref(), &attached_clients(tmux))
}

/// [`ambient_current_session`] for this process, using its `TMUX_PANE`.
pub fn ambient_current_session_for_process(tmux: &tmux_router::Tmux) -> Option<AmbientSession> {
    let current_pane = std::env::var("TMUX_PANE").ok();
    ambient_current_session(tmux, current_pane.as_deref())
}

/// A pane that lives in a tmux session no client is attached to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnwatchedPane {
    pub pane: String,
    pub session: String,
    /// The shell command that attaches a terminal to `session`.
    pub attach_command: String,
    /// Sessions that do have a client attached (where the operator is looking).
    pub watched_sessions: Vec<String>,
}

/// `Some` when `pane` lives in a session with no attached client: whatever
/// runs there is on no screen. `None` when a client shows the pane's session,
/// or the pane's session cannot be read (nothing to report).
pub fn unwatched_pane(tmux: &tmux_router::Tmux, pane: &str) -> Option<UnwatchedPane> {
    let session = tmux.pane_session(pane).ok().filter(|s| !s.is_empty())?;
    let clients = attached_clients(tmux);
    if clients.iter().any(|client| client.session == session) {
        return None;
    }
    let mut watched_sessions = clients
        .into_iter()
        .map(|client| client.session)
        .collect::<Vec<_>>();
    watched_sessions.sort();
    watched_sessions.dedup();
    Some(UnwatchedPane {
        pane: pane.to_string(),
        attach_command: attach_session_command(tmux.server_socket.as_deref(), &session),
        session,
        watched_sessions,
    })
}

/// `tmux attach-session -t <session>`, carrying the isolated server socket when
/// agent-doc talks to one.
pub fn attach_session_command(server_socket: Option<&str>, session: &str) -> String {
    let prefix = server_socket.map_or_else(
        || "tmux".to_string(),
        |socket| format!("tmux -L {} -f /dev/null", shell_word(socket)),
    );
    format!("{prefix} attach-session -t {}", shell_word(session))
}

fn shell_word(value: &str) -> String {
    if !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'_' | b'-' | b'.' | b':' | b'@')
        })
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

/// Stable marker an editor scans a successful route's output for (GH #243).
pub const PANE_NOT_VISIBLE_MARKER: &str = "[route] pane-not-visible:";

/// Operator-facing notice for a dispatch or start into an unwatched pane.
pub fn pane_not_visible_notice(file: &Path, target: &UnwatchedPane) -> String {
    let watched = if target.watched_sessions.is_empty() {
        "no tmux client is attached to any session".to_string()
    } else {
        format!(
            "attached client(s) show session(s) {}",
            target
                .watched_sessions
                .iter()
                .map(|session| format!("'{session}'"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    format!(
        "{PANE_NOT_VISIBLE_MARKER} {} runs in pane {} of tmux session '{}', which has no attached client ({watched}); its harness output is not on any screen. Attach with: {}",
        file.display(),
        target.pane,
        target.session,
        target.attach_command,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(session: &str, activity: u64) -> AttachedTmuxClient {
        AttachedTmuxClient {
            session: session.to_string(),
            activity,
            navigation_lane: false,
        }
    }

    #[test]
    fn attached_ambient_session_is_kept() {
        let choice = choose_ambient_session(Some("0"), &[client("0", 5), client("1", 9)]).unwrap();
        assert_eq!(choice.session, "0");
        assert_eq!(choice.resolution, AmbientSessionResolution::AmbientAttached);
        assert_eq!(choice.passed_over, None);
    }

    /// GH #243: tmux's ambient guess named the client-less session `0` while
    /// the operator's only client was attached to session `1`.
    #[test]
    fn client_less_ambient_session_yields_to_the_attached_session() {
        let choice = choose_ambient_session(Some("0"), &[client("1", 7)]).unwrap();
        assert_eq!(choice.session, "1");
        assert_eq!(
            choice.resolution,
            AmbientSessionResolution::AttachedClientPreferred
        );
        assert_eq!(choice.passed_over.as_deref(), Some("0"));
    }

    #[test]
    fn most_recently_active_attached_client_wins() {
        let choice =
            choose_ambient_session(None, &[client("a", 3), client("b", 11), client("c", 4)])
                .unwrap();
        assert_eq!(choice.session, "b");
    }

    #[test]
    fn navigation_lanes_are_never_switched_into() {
        let lane = AttachedTmuxClient {
            navigation_lane: true,
            ..client("proj-lane", 99)
        };
        let choice = choose_ambient_session(Some("0"), std::slice::from_ref(&lane)).unwrap();
        assert_eq!(choice.session, "0");
        assert_eq!(choice.resolution, AmbientSessionResolution::Unattached);
        // A lane that IS the ambient session and is attached stays put.
        let choice = choose_ambient_session(Some("proj-lane"), &[lane]).unwrap();
        assert_eq!(choice.resolution, AmbientSessionResolution::AmbientAttached);
    }

    #[test]
    fn no_attached_client_keeps_ambient_as_unattached() {
        let choice = choose_ambient_session(Some("0"), &[]).unwrap();
        assert_eq!(choice.session, "0");
        assert_eq!(choice.resolution, AmbientSessionResolution::Unattached);
        assert_eq!(choose_ambient_session(None, &[]), None);
        assert_eq!(choose_ambient_session(Some("  "), &[]), None);
    }

    #[test]
    fn list_clients_output_parses_lane_marker_and_skips_blank_sessions() {
        let parsed = parse_attached_clients("170\t1\t\n180\tlane\t/root\n190\t\t\nbad\t2\t\n");
        assert_eq!(
            parsed,
            vec![
                client("1", 170),
                AttachedTmuxClient {
                    navigation_lane: true,
                    ..client("lane", 180)
                },
                client("2", 0),
            ]
        );
    }

    #[test]
    fn notice_names_session_pane_and_attach_command() {
        let notice = pane_not_visible_notice(
            Path::new("tasks/doc-a.md"),
            &UnwatchedPane {
                pane: "%2".to_string(),
                session: "0".to_string(),
                attach_command: attach_session_command(None, "0"),
                watched_sessions: vec!["1".to_string()],
            },
        );
        assert!(notice.starts_with(PANE_NOT_VISIBLE_MARKER), "{notice}");
        assert!(notice.contains("pane %2 of tmux session '0'"), "{notice}");
        assert!(notice.contains("session(s) '1'"), "{notice}");
        assert!(
            notice.ends_with("Attach with: tmux attach-session -t 0"),
            "{notice}"
        );
        assert_eq!(
            attach_session_command(Some("iso sock"), "my session"),
            "tmux -L 'iso sock' -f /dev/null attach-session -t 'my session'"
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn ambient_lookup_prefers_the_attached_session_and_reports_unwatched_panes() {
        let iso = crate::isolated_test_tmux("gh243-ambient-attached");
        let tmp = std::env::temp_dir();
        let detached_pane = iso.new_session("0", &tmp).unwrap();
        let watched_pane = iso.new_session("1", &tmp).unwrap();
        // Make the client-less session tmux's most recently used one.
        iso.raw_cmd(&["new-window", "-d", "-t", "0:"]).unwrap();

        // No client anywhere: the ambient session stands, unattached.
        let unattached = ambient_current_session(&iso, None).unwrap();
        assert_eq!(unattached.resolution, AmbientSessionResolution::Unattached);
        assert!(unwatched_pane(&iso, &detached_pane).is_some());
        assert!(unwatched_pane(&iso, &watched_pane).is_some());

        let _client = iso.attach_control_mode(Some("1")).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while session_attached_client_count(&iso, "1") == 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "client never attached"
            );
            std::thread::sleep(std::time::Duration::from_millis(25));
        }

        let chosen = ambient_current_session(&iso, None).unwrap();
        assert_eq!(chosen.session, "1");
        assert_ne!(chosen.resolution, AmbientSessionResolution::Unattached);
        assert_eq!(session_attached_client_count(&iso, "0"), 0);

        let unwatched = unwatched_pane(&iso, &detached_pane).unwrap();
        assert_eq!(unwatched.session, "0");
        assert_eq!(unwatched.watched_sessions, vec!["1".to_string()]);
        assert!(unwatched.attach_command.contains("attach-session -t 0"));
        assert_eq!(unwatched_pane(&iso, &watched_pane), None);

        // A caller inside a pane keeps its own pane's session, attached or not.
        let caller = ambient_current_session(&iso, Some(&detached_pane)).unwrap();
        assert_eq!(caller.session, "0");
        assert_eq!(caller.resolution, AmbientSessionResolution::CallerPane);
    }
}
