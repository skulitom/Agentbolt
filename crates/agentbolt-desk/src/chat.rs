//! Validated terminal launch arguments. Windows Terminal treats semicolons as command separators.
use agentbolt_core::navigation::{ChatTarget, valid_session_id};
use std::path::{Path, PathBuf};

fn local_dir(text: &str) -> Option<PathBuf> {
    if text.contains([';', '\'', '"']) || text.chars().any(char::is_control) {
        return None;
    }
    let path = Path::new(text);
    if !path.is_absolute()
        || ["\\\\", "//", "\\/", "/\\"]
            .iter()
            .any(|p| text.starts_with(p))
    {
        return None;
    }
    #[cfg(windows)]
    {
        // Reject mapped network drives before a filesystem lookup can block on the network.
        use windows::Win32::Storage::FileSystem::GetDriveTypeW;
        use windows::core::HSTRING;
        if text.as_bytes().get(1) != Some(&b':') {
            return None;
        }
        let root = format!("{}:\\", &text[..1]);
        // DRIVE_FIXED / DRIVE_REMOVABLE / DRIVE_RAMDISK only.
        if !matches!(unsafe { GetDriveTypeW(&HSTRING::from(root)) }, 2 | 3 | 6) {
            return None;
        }
    }
    let canonical = path.canonicalize().ok()?;
    let normalized = canonical.to_string_lossy();
    let normalized = normalized.strip_prefix("\\\\?\\").unwrap_or(&normalized);
    if normalized.starts_with("UNC\\")
        || normalized.contains([';', '\'', '"'])
        || normalized.chars().any(char::is_control)
    {
        return None;
    }
    let canonical = PathBuf::from(normalized);
    canonical.is_dir().then_some(canonical)
}

pub fn resume_directory(cwd: Option<&str>, home: &str) -> Result<PathBuf, String> {
    cwd.and_then(local_dir)
        .or_else(|| local_dir(home))
        .ok_or_else(|| "No existing local home folder is available for the terminal.".into())
}

/// Every element becomes a separate OS argument; never build a command line or invoke a shell.
pub fn terminal_args(
    target: &ChatTarget,
    cwd: Option<&str>,
    home: &str,
) -> Result<Vec<String>, String> {
    if !valid_session_id(target.id()) {
        return Err("The host supplied an invalid session id.".into());
    }
    let directory = resume_directory(cwd, home)?;
    let (program, resume) = match target {
        ChatTarget::ClaudeTerminal(_) => ("claude", "--resume"),
        ChatTarget::Codex(_) => ("codex", "resume"),
        ChatTarget::ClaudeDesktop(_) => return Err("Desktop chats open in the Claude app.".into()),
    };
    Ok(vec![
        "-d".into(),
        directory.to_string_lossy().into_owned(),
        "--".into(),
        program.into(),
        resume.into(),
        target.id().into(),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID: &str = "01a1503a-5165-7e62-a9ba-88c35a72cfce";

    #[test]
    fn resume_is_an_argument_array_and_unsafe_folders_fall_back() {
        let home = std::env::temp_dir().to_string_lossy().into_owned();
        let expected = resume_directory(None, &home)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        for cwd in [
            "relative",
            "\\\\server\\share",
            "//server/share",
            "C:\\work; calc",
            "C:\\bad\"name",
            "C:\\bad'name",
            "C:\\bad\nname",
            "C:\\missing-agentbolt-folder",
        ] {
            let args =
                terminal_args(&ChatTarget::ClaudeTerminal(ID.into()), Some(cwd), &home).unwrap();
            assert_eq!(args, ["-d", &expected, "--", "claude", "--resume", ID]);
        }
        let args = terminal_args(&ChatTarget::Codex(ID.into()), None, &home).unwrap();
        assert_eq!(&args[2..], &["--", "codex", "resume", ID]);
        for id in ["--help", "abc; calc", "bad\"id", "x\nx"] {
            assert!(terminal_args(&ChatTarget::Codex(id.into()), None, &home).is_err());
        }
        assert!(resume_directory(None, "relative").is_err());
        assert!(resume_directory(Some(env!("CARGO_MANIFEST_DIR")), "relative").is_ok());
    }
}
