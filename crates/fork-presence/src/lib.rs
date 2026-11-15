//! Discord Rich Presence for the fork (docs/fork/DISCORD.md).
//!
//! While the game runs, the player's Discord shows "Playing <application name>" (the name is the
//! Discord application's), a details line with the build and an elapsed-time start timestamp.
//! Nothing about the player, the place, the plugins or the mods is sent, and no game data.
//!
//! The game never waits on this: [`start`] reads the environment, and only when presence is
//! wanted spawns one background thread that connects lazily (Discord's socket or pipe), retries
//! with backoff and stays silent when Discord is not running.

use std::time::Duration;

/// TODO(owner): the Discord application's ID. Create an application named "Fallout: New Rust
/// Vegas" in the Discord developer portal (https://discord.com/developers/applications), copy its
/// Application ID here, or set `NV_DISCORD_APP_ID` at run time. While this is the placeholder,
/// presence stays off.
pub const DEFAULT_APP_ID: &str = "0";

/// What the player sees, coarse on purpose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// The game is starting or showing menus.
    Starting,
    /// The game's frames are running.
    Running,
}

impl State {
    fn text(self) -> &'static str {
        match self {
            State::Starting => "Starting",
            State::Running => "In game",
        }
    }
}

/// Why presence is off.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Off {
    OptedOut,
    PlaceholderAppId,
    InvalidAppId,
    NotBuilt,
}

impl Off {
    pub fn reason(&self) -> &'static str {
        match self {
            Off::OptedOut => "NV_DISCORD=0",
            Off::PlaceholderAppId => {
                "no Discord application ID (set NV_DISCORD_APP_ID; the fork's owner has not supplied one yet)"
            }
            Off::InvalidAppId => "NV_DISCORD_APP_ID is not a Discord application ID (digits only)",
            Off::NotBuilt => "this build has no Discord support",
        }
    }
}

/// The decision from the environment: the application ID to use, or why not.
pub fn decide(get: impl Fn(&str) -> Option<String>) -> Result<String, Off> {
    if let Some(v) = get("NV_DISCORD") {
        let v = v.trim().to_ascii_lowercase();
        if matches!(v.as_str(), "0" | "off" | "false" | "no") {
            return Err(Off::OptedOut);
        }
    }
    let id = get("NV_DISCORD_APP_ID")
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_APP_ID.to_owned());
    if id.is_empty() || id.chars().all(|c| c == '0') {
        return Err(Off::PlaceholderAppId);
    }
    if !id.chars().all(|c| c.is_ascii_digit()) {
        return Err(Off::InvalidAppId);
    }
    Ok(id)
}

/// The activity as plain data, built apart from any connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Payload {
    pub details: String,
    pub state: &'static str,
    /// Unix seconds the run started (Discord shows the elapsed time from it).
    pub start: i64,
}

pub fn payload(version: &str, state: State, start: i64) -> Payload {
    Payload {
        details: format!("Build {version}"),
        state: state.text(),
        start,
    }
}

/// Backoff between connection attempts: 5 s, doubling to 5 minutes.
pub fn backoff(attempt: u32) -> Duration {
    Duration::from_secs((5u64 << attempt.min(6)).min(300))
}

/// Handle to the background thread; dropping it ends the thread. Calls never block.
pub struct Presence {
    #[cfg(all(feature = "discord", any(windows, target_os = "linux")))]
    tx: std::sync::mpsc::Sender<State>,
}

impl Presence {
    /// Tell Discord the coarse state changed.
    pub fn set_state(&self, state: State) {
        #[cfg(all(feature = "discord", any(windows, target_os = "linux")))]
        {
            let _ = self.tx.send(state);
        }
        #[cfg(not(all(feature = "discord", any(windows, target_os = "linux"))))]
        {
            let _ = state;
        }
    }
}

/// Start presence from the environment. `None` (with one info line saying why) when off.
pub fn start(version: &str) -> Option<Presence> {
    start_with(version, |name| std::env::var(name).ok())
}

fn start_with(version: &str, get: impl Fn(&str) -> Option<String>) -> Option<Presence> {
    let id = match decide(get) {
        Ok(id) => id,
        Err(off) => {
            eprintln!("info: Discord presence is off: {}", off.reason());
            return None;
        }
    };
    spawn(id, version)
}

#[cfg(all(feature = "discord", any(windows, target_os = "linux")))]
fn spawn(id: String, version: &str) -> Option<Presence> {
    use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
    use std::sync::mpsc::{channel, RecvTimeoutError};
    use std::time::{SystemTime, UNIX_EPOCH};

    let (tx, rx) = channel::<State>();
    let version = version.to_owned();
    let start = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let spawned = std::thread::Builder::new()
        .name("discord-presence".into())
        .spawn(move || {
            let mut state = State::Starting;
            let mut attempt = 0u32;
            'outer: loop {
                // Pick up the latest state; a closed channel means the game is ending.
                let mut client = DiscordIpcClient::new(&id);
                let connected = client.connect().is_ok();
                if connected {
                    attempt = 0;
                    loop {
                        let p = payload(&version, state, start);
                        let act = activity::Activity::new()
                            .details(&p.details)
                            .state(p.state)
                            .timestamps(activity::Timestamps::new().start(p.start));
                        if client.set_activity(act).is_err() {
                            break;
                        }
                        match rx.recv() {
                            Ok(s) => state = s,
                            Err(_) => {
                                let _ = client.close();
                                break 'outer;
                            }
                        }
                    }
                    let _ = client.close();
                }
                // Discord absent or the link broke: wait, then try again (silently).
                match rx.recv_timeout(backoff(attempt)) {
                    Ok(s) => state = s,
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                }
                attempt = attempt.saturating_add(1);
            }
        });
    match spawned {
        Ok(_) => Some(Presence { tx }),
        Err(_) => None,
    }
}

#[cfg(not(all(feature = "discord", any(windows, target_os = "linux"))))]
fn spawn(_id: String, _version: &str) -> Option<Presence> {
    eprintln!("info: Discord presence is off: {}", Off::NotBuilt.reason());
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| {
            pairs
                .iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| (*v).to_owned())
        }
    }

    #[test]
    fn placeholder_id_keeps_presence_off() {
        assert_eq!(decide(env(&[])), Err(Off::PlaceholderAppId));
        assert_eq!(
            decide(env(&[("NV_DISCORD_APP_ID", "  ")])),
            Err(Off::PlaceholderAppId)
        );
    }

    #[test]
    fn opt_out_wins_over_a_real_id() {
        for v in ["0", "off", "False", " no "] {
            assert_eq!(
                decide(env(&[
                    ("NV_DISCORD", v),
                    ("NV_DISCORD_APP_ID", "123456789")
                ])),
                Err(Off::OptedOut)
            );
        }
    }

    #[test]
    fn id_from_the_environment_is_used() {
        assert_eq!(
            decide(env(&[("NV_DISCORD_APP_ID", "1234567890")])),
            Ok("1234567890".to_owned())
        );
        assert_eq!(
            decide(env(&[("NV_DISCORD", "1"), ("NV_DISCORD_APP_ID", "42")])),
            Ok("42".to_owned())
        );
        assert_eq!(
            decide(env(&[("NV_DISCORD_APP_ID", "abc")])),
            Err(Off::InvalidAppId)
        );
    }

    #[test]
    fn payload_has_build_state_and_start_only() {
        let p = payload("0.1.0", State::Running, 1_700_000_000);
        assert_eq!(p.details, "Build 0.1.0");
        assert_eq!(p.state, "In game");
        assert_eq!(p.start, 1_700_000_000);
        assert_eq!(payload("0.1.0", State::Starting, 0).state, "Starting");
    }

    #[test]
    fn backoff_doubles_and_caps() {
        assert_eq!(backoff(0), Duration::from_secs(5));
        assert_eq!(backoff(1), Duration::from_secs(10));
        assert_eq!(backoff(6), Duration::from_secs(300));
        assert_eq!(backoff(1000), Duration::from_secs(300));
    }

    #[test]
    fn disabled_paths_start_nothing() {
        assert!(start_with("0.1.0", env(&[])).is_none());
        assert!(start_with("0.1.0", env(&[("NV_DISCORD", "0")])).is_none());
    }
}
