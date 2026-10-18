//! Per-channel names (PLAN.md, "A test channel"). The real install is channel `main`. Tests set
//! `AGENTBOLT_CHANNEL=<name>` so nothing they register collides with Artem's install, and remove it afterwards.

use std::path::PathBuf;

pub const ENV: &str = "AGENTBOLT_CHANNEL";
pub const MAIN: &str = "main";

/// Every per-user name Agentbolt registers, derived from one channel name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Names {
    pub channel: String,
    /// Windows AppUserModelID for toasts.
    pub aumid: String,
    /// URL scheme for toast buttons, e.g. `agentbolt` or `agentbolt-test1`.
    pub protocol: String,
    /// Task Scheduler task name.
    pub task: String,
    /// Named pipe for `agentbolt mcp` clients (Windows: `\\.\pipe\<name>`).
    pub mcp_pipe: String,
    /// Named pipe for the Desk window and the protocol handler.
    pub desk_pipe: String,
    /// Single-instance mutex (`Global\` prefix added on Windows).
    pub mutex: String,
}

impl Names {
    pub fn for_channel(channel: &str) -> Result<Self, String> {
        let valid = !channel.is_empty()
            && channel.len() <= 32
            && channel.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            && !channel.starts_with('-');
        if !valid {
            return Err(format!(
                "{ENV} takes 1-32 lowercase letters, digits or hyphens, such as test1, not '{channel}'"
            ));
        }
        let main = channel == MAIN;
        let suffix = if main { String::new() } else { format!("-{channel}") };
        Ok(Self {
            channel: channel.to_string(),
            aumid: if main { "Skulitom.Agentbolt".into() } else { format!("Skulitom.Agentbolt.{channel}") },
            protocol: format!("agentbolt{suffix}"),
            task: if main { "Agentbolt".into() } else { format!("Agentbolt ({channel})") },
            mcp_pipe: format!("agentbolt{suffix}-mcp"),
            desk_pipe: format!("agentbolt{suffix}-desk"),
            mutex: format!("Agentbolt{suffix}-daemon"),
        })
    }

    /// The channel from the environment, or `main`.
    pub fn from_env() -> Result<Self, String> {
        let channel = std::env::var(ENV).unwrap_or_else(|_| MAIN.to_string());
        Self::for_channel(channel.trim())
    }

    pub fn is_main(&self) -> bool {
        self.channel == MAIN
    }

    /// Data directory: `%LOCALAPPDATA%\Agentbolt\<channel>` on Windows, `$XDG_DATA_HOME/agentbolt/<channel>` elsewhere.
    pub fn data_dir(&self) -> PathBuf {
        base_data_dir().join(&self.channel)
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir().join("agentbolt.db")
    }
}

fn base_data_dir() -> PathBuf {
    if cfg!(windows) {
        let local = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
        PathBuf::from(local).join("Agentbolt")
    } else {
        let base = std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".local/share")
            });
        base.join("agentbolt")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_and_test_channels_never_share_a_name() {
        let main = Names::for_channel("main").unwrap();
        let test = Names::for_channel("test1").unwrap();
        assert_eq!(main.protocol, "agentbolt");
        assert_eq!(test.protocol, "agentbolt-test1");
        assert_ne!(main.aumid, test.aumid);
        assert_ne!(main.task, test.task);
        assert_ne!(main.mcp_pipe, test.mcp_pipe);
        assert_ne!(main.desk_pipe, test.desk_pipe);
        assert_ne!(main.mutex, test.mutex);
        assert_ne!(main.data_dir(), test.data_dir());
    }

    #[test]
    fn bad_channel_names_are_refused() {
        for bad in ["", "Test", "a b", "-x", "x/y", &"a".repeat(33)] {
            assert!(Names::for_channel(bad).is_err(), "{bad:?} should be refused");
        }
    }
}
