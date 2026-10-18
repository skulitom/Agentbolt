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
    /// Historical Task Scheduler name; Windows now qualifies it with the process token's SID.
    pub task: String,
    /// Endpoint discovery key for `agentbolt mcp` clients (Windows resolves a private random pipe).
    pub mcp_pipe: String,
    /// Named pipe for the Desk window and the protocol handler.
    pub desk_pipe: String,
    /// Instance coordination key (a private profile lock file on Windows).
    pub mutex: String,
}

impl Names {
    pub fn for_channel(channel: &str) -> Result<Self, String> {
        let valid = !channel.is_empty()
            && channel.len() <= 32
            && channel
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            && !channel.starts_with('-');
        if !valid {
            return Err(format!(
                "{ENV} takes 1-32 lowercase letters, digits or hyphens, such as test1, not '{channel}'"
            ));
        }
        let main = channel == MAIN;
        let suffix = if main {
            String::new()
        } else {
            format!("-{channel}")
        };
        Ok(Self {
            channel: channel.to_string(),
            aumid: if main {
                "Skulitom.Agentbolt".into()
            } else {
                format!("Skulitom.Agentbolt.{channel}")
            },
            protocol: format!("agentbolt{suffix}"),
            task: if main {
                "Agentbolt".into()
            } else {
                format!("Agentbolt ({channel})")
            },
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

    /// Data directory: `%LOCALAPPDATA%\Agentbolt\<channel>` on Windows, `$XDG_DATA_HOME/agentbolt/<channel>`
    /// (else `$HOME/.local/share/agentbolt/<channel>`) elsewhere. An error, never a relative fallback, when the
    /// variable is unset or not an absolute path: callers create and delete this folder.
    pub fn data_dir(&self) -> Result<PathBuf, String> {
        Ok(base_data_dir()?.join(&self.channel))
    }

    pub fn db_path(&self) -> Result<PathBuf, String> {
        Ok(self.data_dir()?.join("agentbolt.db"))
    }
}

/// The absolute folder an environment variable names. Unset, empty or relative values are refused with a reason,
/// so nothing ever falls back to the current folder.
pub fn env_dir(var: &str) -> Result<PathBuf, String> {
    let value = std::env::var_os(var).filter(|v| !v.is_empty());
    let Some(value) = value else {
        return Err(format!(
            "{var} isn't set, so Agentbolt can't tell where its folders go; set it and try again"
        ));
    };
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err(format!(
            "{var} is {}, which isn't an absolute path; Agentbolt won't use it",
            path.display()
        ));
    }
    Ok(path)
}

fn base_data_dir() -> Result<PathBuf, String> {
    if cfg!(windows) {
        Ok(env_dir("LOCALAPPDATA")?.join("Agentbolt"))
    } else {
        let base = match env_dir("XDG_DATA_HOME") {
            Ok(dir) => dir,
            Err(_) => env_dir("HOME")?.join(".local").join("share"),
        };
        Ok(base.join("agentbolt"))
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
        assert_ne!(main.data_dir().unwrap(), test.data_dir().unwrap());
        assert!(test.data_dir().unwrap().is_absolute());
    }

    #[test]
    fn folders_from_the_environment_must_be_absolute() {
        // Read-only checks on variables no other test sets.
        assert!(
            env_dir("AGENTBOLT_TEST_SURELY_UNSET_VAR")
                .unwrap_err()
                .contains("isn't set")
        );
        let var = if cfg!(windows) {
            "LOCALAPPDATA"
        } else {
            "HOME"
        };
        assert!(env_dir(var).unwrap().is_absolute());
    }

    #[test]
    fn bad_channel_names_are_refused() {
        for bad in ["", "Test", "a b", "-x", "x/y", &"a".repeat(33)] {
            assert!(
                Names::for_channel(bad).is_err(),
                "{bad:?} should be refused"
            );
        }
    }
}
