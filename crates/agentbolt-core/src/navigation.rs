//! Host-derived chat destinations. Never select a destination from an ask's evidence or body.

pub const CLAUDE_DESKTOP_PREFIX: &str = "claude://claude.ai/epitaxy/local_";

/// Accepted resume arguments: a UUID or 16–64 hexadecimal characters. No switches or shell syntax.
pub fn valid_session_id(id: &str) -> bool {
    let hex = |b: u8| b.is_ascii_hexdigit();
    ((16..=64).contains(&id.len()) && id.bytes().all(hex))
        || (id.len() == 36
            && id.bytes().enumerate().all(|(i, b)| {
                if [8, 13, 18, 23].contains(&i) {
                    b == b'-'
                } else {
                    hex(b)
                }
            }))
}

pub fn claude_desktop_ref(host: &str) -> Option<String> {
    let id = host.strip_prefix("local_")?;
    valid_session_id(id).then(|| format!("{CLAUDE_DESKTOP_PREFIX}{id}"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatTarget {
    ClaudeDesktop(String),
    ClaudeTerminal(String),
    Codex(String),
}

impl ChatTarget {
    pub fn from_host(client: &str, session: Option<&str>) -> Option<Self> {
        let session = session?;
        if let Some(id) = session.strip_prefix(CLAUDE_DESKTOP_PREFIX) {
            return valid_session_id(id).then(|| Self::ClaudeDesktop(id.into()));
        }
        if !valid_session_id(session) {
            return None;
        }
        let client = client.to_ascii_lowercase();
        if client.contains("codex") {
            Some(Self::Codex(session.into()))
        } else if client.contains("claude") {
            Some(Self::ClaudeTerminal(session.into()))
        } else {
            None
        }
    }

    pub fn id(&self) -> &str {
        match self {
            Self::ClaudeDesktop(id) | Self::ClaudeTerminal(id) | Self::Codex(id) => id,
        }
    }

    pub fn label(&self) -> &'static str {
        if matches!(self, Self::ClaudeTerminal(_)) {
            "Resume in a terminal"
        } else {
            "Open chat"
        }
    }

    /// A command the user can paste into an existing terminal. This never includes agent-supplied
    /// folders or shell syntax, and is not executed by the Desk.
    pub fn resume_command(&self) -> Option<String> {
        if !valid_session_id(self.id()) {
            return None;
        }
        match self {
            Self::ClaudeTerminal(id) => Some(format!("claude --resume {id}")),
            Self::Codex(id) => Some(format!("codex resume {id}")),
            Self::ClaudeDesktop(_) => None,
        }
    }

    /// Rebuild the URL from a validated id even if a caller constructed the enum directly.
    pub fn url(&self) -> Option<String> {
        if !valid_session_id(self.id()) {
            return None;
        }
        match self {
            Self::ClaudeDesktop(id) => Some(format!("{CLAUDE_DESKTOP_PREFIX}{id}")),
            Self::Codex(id) => Some(format!("codex://threads/{id}")),
            Self::ClaudeTerminal(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID: &str = "01a1503a-5165-7e62-a9ba-88c35a72cfce";

    #[test]
    fn destinations_are_built_only_from_validated_host_identifiers() {
        let desktop = claude_desktop_ref(&format!("local_{ID}")).unwrap();
        assert_eq!(
            ChatTarget::from_host("claude-code", Some(&desktop)),
            Some(ChatTarget::ClaudeDesktop(ID.into()))
        );
        assert_eq!(
            ChatTarget::from_host("claude-code", Some(ID)),
            Some(ChatTarget::ClaudeTerminal(ID.into()))
        );
        assert_eq!(
            ChatTarget::from_host("Codex", Some(ID))
                .unwrap()
                .url()
                .unwrap(),
            format!("codex://threads/{ID}")
        );
        assert!(ChatTarget::from_host("unknown", Some(ID)).is_none());
        for bad in [
            "",
            "--help",
            "abc;calc",
            "abcd\"",
            "$(whoami)",
            "id\n",
            "https://example.com",
            "claude://elsewhere/session",
            "01a1503a-5165-7e62-a9ba-88c35a72cfce/extra",
        ] {
            assert!(!valid_session_id(bad), "{bad}");
            assert!(ChatTarget::from_host("codex", Some(bad)).is_none());
            assert!(ChatTarget::Codex(bad.into()).url().is_none());
            assert!(ChatTarget::Codex(bad.into()).resume_command().is_none());
            assert!(
                ChatTarget::ClaudeTerminal(bad.into())
                    .resume_command()
                    .is_none()
            );
            assert!(claude_desktop_ref(&format!("local_{bad}")).is_none());
        }
        assert!(valid_session_id("a1b2c3d4e5f60708"));
        assert!(!valid_session_id("--------5165-7e62-a9ba-88c35a72cfce"));
        assert!(claude_desktop_ref(ID).is_none());
        assert_eq!(
            ChatTarget::ClaudeTerminal(ID.into())
                .resume_command()
                .as_deref(),
            Some(format!("claude --resume {ID}").as_str())
        );
        assert_eq!(
            ChatTarget::Codex(ID.into()).resume_command().as_deref(),
            Some(format!("codex resume {ID}").as_str())
        );
        assert!(
            ChatTarget::ClaudeDesktop(ID.into())
                .resume_command()
                .is_none()
        );
    }
}
