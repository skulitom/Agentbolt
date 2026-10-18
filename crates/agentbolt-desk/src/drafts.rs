//! Local, unsent UI state. Loading a draft never submits an answer.
//! Each Windows session has its own file; a new session may restore the latest saved drafts.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::view::FieldInput;

const MAX_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Draft {
    pub choice: Option<String>,
    pub note: String,
    pub text: String,
    /// Clearing a prefilled reply must not fill it again when the Desk restarts.
    pub text_filled: bool,
    pub fields: Option<HashMap<String, FieldInput>>,
}

#[derive(Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Snapshot {
    pub drafts: HashMap<String, Draft>,
    pub selected: Option<String>,
    pub batch: HashSet<String>,
}

#[derive(Serialize, Deserialize)]
struct File {
    version: u32,
    state: Snapshot,
}

pub struct Store {
    path: PathBuf,
    saved: Snapshot,
    load_error: Option<String>,
}

pub fn session_id() -> u32 {
    #[cfg(windows)]
    {
        use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
        let mut id = 0;
        if unsafe { ProcessIdToSessionId(std::process::id(), &mut id) }.is_ok() {
            return id;
        }
    }
    0
}

impl Store {
    pub fn load(dir: &Path) -> (Self, Snapshot) {
        let path = dir.join(format!("desk-drafts-{}.json", session_id()));
        // A new sign-in may receive another Windows session id. Do not overwrite another session's file.
        let source = if path.exists() {
            Some(path.clone())
        } else {
            std::fs::read_dir(dir)
                .ok()
                .into_iter()
                .flatten()
                .flatten()
                .filter(|entry| is_draft_file(&entry.file_name().to_string_lossy()))
                .filter_map(|entry| {
                    let meta = entry.metadata().ok()?;
                    meta.is_file().then(|| (meta.modified().ok(), entry.path()))
                })
                .max_by_key(|(time, _)| *time)
                .map(|(_, path)| path)
        };
        let loaded = source.as_deref().map(read).transpose();
        let (state, error) = match loaded {
            Ok(state) => (state.unwrap_or_default(), None),
            Err(error) => (Snapshot::default(), Some(error)),
        };
        (
            Self {
                path,
                saved: state.clone(),
                load_error: error,
            },
            state,
        )
    }

    pub fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    /// A failed load is never replaced with an empty/new file. A failed write leaves the previous file intact.
    pub fn save(&mut self, state: Snapshot) -> Result<(), String> {
        if let Some(error) = &self.load_error {
            return Err(error.clone());
        }
        if state == self.saved {
            return Ok(());
        }
        let bytes = serde_json::to_vec(&File {
            version: 1,
            state: state.clone(),
        })
        .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err("Unsent drafts exceed the 4 MiB local storage limit".into());
        }
        write_atomic(&self.path, &bytes)
            .map_err(|e| format!("Couldn't save drafts to {}: {e}", self.path.display()))?;
        self.saved = state;
        Ok(())
    }
}

fn is_draft_file(name: &str) -> bool {
    name.strip_prefix("desk-drafts-")
        .and_then(|n| n.strip_suffix(".json"))
        .is_some_and(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
}

fn read(path: &Path) -> Result<Snapshot, String> {
    let result = (|| -> Result<Snapshot, String> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|e| e.to_string())?
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err("draft file exceeds 4 MiB".into());
        }
        let file: File = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if file.version != 1 {
            return Err(format!("unsupported draft version {}", file.version));
        }
        Ok(file.state)
    })();
    result.map_err(|e| {
        format!(
            "Couldn't restore drafts from {}: {e}. The existing file has been kept.",
            path.display()
        )
    })
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::other("missing draft directory"))?;
    std::fs::create_dir_all(dir)?;
    let temporary = dir.join(format!(
        ".desk-drafts-{}-{}.tmp",
        std::process::id(),
        agentbolt_core::tokens::random_hex(8)
    ));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Temp(PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn temp() -> Temp {
        let path = std::env::temp_dir().join(format!(
            "agentbolt-drafts-{}",
            agentbolt_core::tokens::random_hex(8)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Temp(path)
    }

    #[test]
    fn drafts_restore_edits_cleared_replies_batch_selection_and_removal() {
        let dir = temp();
        let (mut store, _) = Store::load(&dir.0);
        let mut state = Snapshot {
            selected: Some("ask_one".into()),
            ..Snapshot::default()
        };
        state.batch.insert("ask_one".into());
        state.drafts.insert(
            "ask_one".into(),
            Draft {
                choice: Some("No".into()),
                note: "Unsent note".into(),
                text: String::new(),
                text_filled: true,
                fields: Some(HashMap::from([
                    ("quantity".into(), FieldInput::Text("7".into())),
                    ("checked".into(), FieldInput::Bool(true)),
                    ("unanswered".into(), FieldInput::OptionalBool(None)),
                    ("declined".into(), FieldInput::OptionalBool(Some(false))),
                ])),
            },
        );
        store.save(state.clone()).unwrap();
        let (_, restored) = Store::load(&dir.0);
        assert!(state == restored);
        state.drafts.clear();
        state.batch.clear();
        state.selected = None;
        store.save(state).unwrap();
        assert!(Store::load(&dir.0).1.drafts.is_empty());
        assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 1);
    }

    #[test]
    fn damaged_and_future_drafts_are_not_overwritten() {
        let dir = temp();
        let path = dir.0.join(format!("desk-drafts-{}.json", session_id()));
        for text in ["{broken", r#"{"version":999,"state":{}}"#] {
            std::fs::write(&path, text).unwrap();
            let (mut store, state) = Store::load(&dir.0);
            assert!(store.load_error().is_some());
            assert!(store.save(state).is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        }
    }

    #[test]
    fn a_new_session_restores_without_overwriting_the_previous_sessions_file() {
        let dir = temp();
        let previous = dir.0.join(format!("desk-drafts-{}.json", u32::MAX));
        let text = r#"{"version":1,"state":{"drafts":{"ask_saved":{"note":"Keep me"}}}}"#;
        std::fs::write(&previous, text).unwrap();
        let (mut store, mut state) = Store::load(&dir.0);
        assert_eq!(state.drafts["ask_saved"].note, "Keep me");
        state.drafts.get_mut("ask_saved").unwrap().note = "Edited here".into();
        store.save(state).unwrap();
        assert_eq!(std::fs::read_to_string(previous).unwrap(), text);
    }
}
