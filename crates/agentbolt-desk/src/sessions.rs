//! Read-only discovery of local Codex and Claude sessions. No messages, credentials or app writes.
//! Scans run separately from both rendering and the answer pipe worker.

use std::collections::HashSet;
use std::fs::{self, File, TryLockError};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use agentbolt_core::channel::Names;
use agentbolt_core::navigation::{ChatTarget, claude_desktop_ref, valid_session_id};
use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;

const RECENT_LIMIT: usize = 200;
const ENTRY_LIMIT: usize = 4096;

#[derive(Debug, Clone)]
pub struct Session {
    pub target: ChatTarget,
    pub title: String,
    pub cwd: String,
    pub updated: i64,
    /// A held Codex writer or a verified, still-running Claude process. Not evidence of a running turn.
    pub open: bool,
    pub background: bool,
    pub status: &'static str,
}

impl Session {
    pub fn app(&self) -> &'static str {
        match self.target {
            ChatTarget::Codex(_) => "Codex",
            ChatTarget::ClaudeDesktop(_) => "Claude Desktop",
            ChatTarget::ClaudeTerminal(_) => "Claude Code",
        }
    }

    pub fn matches(&self, search: &str) -> bool {
        search.split_whitespace().all(|word| {
            self.title.to_lowercase().contains(word)
                || self.cwd.to_lowercase().contains(word)
                || self.app().to_lowercase().contains(word)
                || self.target.id().to_lowercase().contains(word)
        })
    }
}

#[derive(Debug, Default)]
pub struct Snapshot {
    pub items: Vec<Session>,
    pub warnings: Vec<String>,
}

pub struct Worker {
    tx: mpsc::SyncSender<()>,
    rx: mpsc::Receiver<Snapshot>,
}

impl Worker {
    pub fn spawn(names: &Names, wake: impl Fn() + Send + 'static) -> Self {
        let roots = Roots::for_channel(names);
        let (tx, requests) = mpsc::sync_channel(1);
        let (results, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("session-discovery".into())
            .spawn(move || {
                while requests.recv().is_ok() {
                    let snapshot = roots.as_ref().map_or_else(
                        || Snapshot {
                            warnings: vec![
                                "Local session discovery is unavailable for this profile.".into(),
                            ],
                            ..Default::default()
                        },
                        discover,
                    );
                    if results.send(snapshot).is_err() {
                        break;
                    }
                    wake();
                }
            })
            .expect("the session discovery thread");
        Self { tx, rx }
    }

    pub fn refresh(&self) -> bool {
        self.tx.try_send(()).is_ok()
    }

    pub fn latest(&self) -> Option<Snapshot> {
        self.rx.try_iter().last()
    }
}

struct Roots {
    codex: PathBuf,
    claude: PathBuf,
}

impl Roots {
    fn for_channel(names: &Names) -> Option<Self> {
        // Isolated previews never discover real chats by accident.
        if !names.is_main() {
            let home = std::env::var_os("AGENTBOLT_SESSION_PREVIEW_ROOT")?;
            let home = local_path(Path::new(&home))?;
            return Some(Self {
                codex: home.join(".codex"),
                claude: home.join(".claude"),
            });
        }
        let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })?;
        let home = local_path(Path::new(&home))?;
        let codex = std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".codex"));
        let claude = std::env::var_os("CLAUDE_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".claude"));
        Some(Self { codex, claude })
    }
}

/// Refuse network locations and reparse escapes before opening a provider file.
fn local_path(path: &Path) -> Option<PathBuf> {
    checked_local_path(path).ok().flatten()
}

fn checked_local_path(path: &Path) -> Result<Option<PathBuf>, String> {
    let text = path.to_string_lossy();
    if !path.is_absolute() || text.starts_with(['\\', '/']) && cfg!(windows) {
        return Err("session folders must use an absolute local path".into());
    }
    #[cfg(windows)]
    {
        use windows::Win32::Storage::FileSystem::GetDriveTypeW;
        use windows::core::HSTRING;
        if text.as_bytes().get(1) != Some(&b':') {
            return Err("session folders must use a local drive".into());
        }
        let root = format!("{}:\\", &text[..1]);
        if !matches!(unsafe { GetDriveTypeW(&HSTRING::from(root)) }, 2 | 3 | 6) {
            return Err("network session folders are not supported".into());
        }
    }
    // Checking each existing component avoids following a junction onto an unavailable share.
    let mut current = PathBuf::new();
    for part in path.components() {
        current.push(part);
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
        if metadata.file_type().is_symlink() {
            return Err("linked session folders are not supported".into());
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err("linked session folders are not supported".into());
            }
        }
    }
    Ok(Some(path.to_path_buf()))
}

fn discover(roots: &Roots) -> Snapshot {
    let mut result = Snapshot::default();
    if let Err(error) = codex_sessions(&roots.codex, &mut result) {
        result
            .warnings
            .push(format!("Codex sessions couldn't be read: {error}"));
    }
    if let Err(error) = claude_sessions(&roots.claude, &mut result) {
        result
            .warnings
            .push(format!("Claude sessions couldn't be read: {error}"));
    }
    result.items.sort_by(|a, b| {
        b.open
            .cmp(&a.open)
            .then(b.updated.cmp(&a.updated))
            .then(a.title.cmp(&b.title))
    });
    // A resumed Claude chat may briefly have both an old and a new registry entry.
    // Prefer the verified open process, then the newest metadata, instead of directory order.
    let mut seen = HashSet::new();
    result
        .items
        .retain(|item| seen.insert((item.app(), item.target.id().to_owned())));
    result.warnings.sort();
    result.warnings.dedup();
    result
}

fn codex_sessions(root: &Path, result: &mut Snapshot) -> Result<(), String> {
    let Some(root) = checked_local_path(root)? else {
        return Ok(());
    };
    // Select the newest numbered state database, never logs, message history or credentials.
    let mut databases = Vec::new();
    for entry in fs::read_dir(&root)
        .map_err(|e| e.to_string())?
        .take(ENTRY_LIMIT)
    {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(version) = name
            .strip_prefix("state_")
            .and_then(|s| s.strip_suffix(".sqlite"))
            .and_then(|s| s.parse::<u32>().ok())
        {
            databases.push((version, entry.path()));
        }
    }
    databases.sort_by_key(|(version, _)| std::cmp::Reverse(*version));
    let Some((_, database)) = databases.first() else {
        return Ok(());
    };
    let database = local_path(database).ok_or("the session index is not a local file")?;
    let db = Connection::open_with_flags(
        database,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| e.to_string())?;
    db.busy_timeout(Duration::from_millis(150))
        .map_err(|e| e.to_string())?;
    db.pragma_update(None, "query_only", true)
        .map_err(|e| e.to_string())?;
    let locks = root.join("thread-writer-locks");
    let mut open_ids = HashSet::new();
    if let Some(locks) = checked_local_path(&locks)? {
        for (i, entry) in fs::read_dir(&locks).map_err(|e| e.to_string())?.enumerate() {
            if i >= ENTRY_LIMIT {
                result
                    .warnings
                    .push("Only the first 4,096 Codex session markers could be inspected.".into());
                break;
            }
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(id) = name.strip_suffix(".lock").filter(|id| valid_session_id(id)) else {
                continue;
            };
            match codex_open(&locks, &entry.path()) {
                Some(true) => { open_ids.insert(id.to_owned()); }
                Some(false) => {}
                None => result.warnings.push("Some Codex open-session states are unavailable. Those chats may appear under Recent chats.".into()),
            }
        }
    }
    let mut schema = db
        .prepare("PRAGMA table_info(threads)")
        .map_err(|e| e.to_string())?;
    let fields: HashSet<String> = schema
        .query_map([], |row| row.get(1))
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<_>>()
        .map_err(|e| e.to_string())?;
    // The app's visible name takes precedence over the original first-message title.
    let name = if fields.contains("name") {
        "NULLIF(name, '')"
    } else {
        "NULL"
    };
    let nickname = if fields.contains("agent_nickname") {
        "NULLIF(agent_nickname, '')"
    } else {
        "NULL"
    };
    let columns = format!(
        "id, substr(COALESCE({name}, {nickname}, title), 1, 240), substr(cwd, 1, 4096), updated_at, source"
    );
    let mut recent = db.prepare(&format!("SELECT {columns} FROM threads WHERE archived = 0 ORDER BY updated_at DESC, id DESC LIMIT ?1")).map_err(|e| e.to_string())?;
    let rows = recent
        .query_map([RECENT_LIMIT as i64], codex_row)
        .map_err(|e| e.to_string())?;
    let mut ids = HashSet::new();
    for row in rows {
        let mut item = row.map_err(|e| e.to_string())?;
        if !valid_session_id(item.target.id()) {
            continue;
        }
        item.open = open_ids.contains(item.target.id());
        item.status = if item.open { "Open" } else { "Recent" };
        ids.insert(item.target.id().to_owned());
        result.items.push(item);
    }
    // Include open chats older than the recent-page bound.
    let mut by_id = db
        .prepare(&format!(
            "SELECT {columns} FROM threads WHERE id = ?1 AND archived = 0"
        ))
        .map_err(|e| e.to_string())?;
    for id in open_ids.difference(&ids) {
        match by_id.query_row([id], codex_row) {
            Ok(mut item) => {
                item.open = true;
                item.status = "Open";
                result.items.push(item);
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}

fn codex_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Session> {
    let source: String = row.get(4)?;
    let title: String = row.get(1)?;
    Ok(Session {
        target: ChatTarget::Codex(row.get(0)?),
        title: clean_title(&title, "Codex chat"),
        cwd: row.get(2)?,
        updated: row.get(3)?,
        open: false,
        background: source.contains("subagent"),
        status: "Recent",
    })
}

fn codex_open(directory: &Path, path: &Path) -> Option<bool> {
    // Codex takes this coordination lock before creating/removing a writer. Hold it shared while
    // probing, so our momentary read lock cannot cause a racing writer to report a false conflict.
    // Never create, truncate, remove, or wait on either lock file.
    let coordination = File::open(local_path(&directory.join(".coordination.lock"))?).ok()?;
    coordination.try_lock_shared().ok()?;
    let file = File::open(local_path(path)?).ok()?;
    match file.try_lock_shared() {
        Ok(()) => Some(false),
        Err(TryLockError::WouldBlock) => Some(true),
        Err(TryLockError::Error(_)) => None,
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeSession {
    pid: u32,
    session_id: String,
    #[serde(default)]
    cwd: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    host_session_id: Option<String>,
    #[serde(default)]
    entrypoint: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    proc_start: Option<String>,
    #[serde(default)]
    updated_at: i64,
}

fn claude_sessions(root: &Path, result: &mut Snapshot) -> Result<(), String> {
    let Some(root) = checked_local_path(root)? else {
        return Ok(());
    };
    let dir = root.join("sessions");
    let Some(dir) = checked_local_path(&dir)? else {
        return Ok(());
    };
    for (i, entry) in fs::read_dir(dir).map_err(|e| e.to_string())?.enumerate() {
        if i >= ENTRY_LIMIT {
            result
                .warnings
                .push("Only the first 4,096 Claude session entries could be inspected.".into());
            break;
        }
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(file_pid) = name
            .strip_suffix(".json")
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        let Some(path) = local_path(&entry.path()) else {
            continue;
        };
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                result
                    .warnings
                    .push(format!("A Claude session entry couldn't be read: {error}"));
                continue;
            }
        };
        if file.metadata().map_err(|e| e.to_string())?.len() > 64 * 1024 {
            continue;
        }
        let mut bytes = Vec::new();
        file.take(64 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        let Ok(session) = serde_json::from_slice::<ClaudeSession>(&bytes) else {
            continue;
        };
        if session.pid != file_pid || !valid_session_id(&session.session_id) {
            continue;
        }
        let open = claude_process_matches(session.pid, session.proc_start.as_deref());
        if open.is_none() {
            result.warnings.push(
                "Some Claude process states couldn't be verified. They appear under Recent chats."
                    .into(),
            );
        }
        let open = open.unwrap_or(false);
        let desktop = session
            .host_session_id
            .as_deref()
            .and_then(claude_desktop_ref);
        let target = if session.entrypoint == "claude-desktop" {
            let Some(reference) = desktop else {
                continue;
            };
            let Some(target) = ChatTarget::from_host("claude", Some(&reference)) else {
                continue;
            };
            target
        } else {
            ChatTarget::ClaudeTerminal(session.session_id)
        };
        let status = if !open {
            "Recent"
        } else {
            match session.status.as_str() {
                "idle" => "Open · idle",
                "waiting" => "Open · waiting",
                "working" | "running" | "busy" => "Working",
                _ => "Open",
            }
        };
        result.items.push(Session {
            target,
            title: clean_title(&session.name, "Claude chat"),
            cwd: session.cwd,
            updated: session.updated_at / 1000,
            open,
            background: !session.kind.is_empty() && session.kind != "interactive",
            status,
        });
    }
    Ok(())
}

fn clean_title(value: &str, fallback: &str) -> String {
    let title: String = value
        .chars()
        .filter(|c| !c.is_control())
        .take(240)
        .collect();
    if title.trim().is_empty() {
        fallback.into()
    } else {
        title
    }
}

#[cfg(windows)]
fn claude_process_matches(pid: u32, start: Option<&str>) -> Option<bool> {
    use windows::Win32::Foundation::{CloseHandle, FILETIME};
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };
    use windows::core::PWSTR;
    let expected = start?.parse::<u64>().ok()?;
    unsafe {
        let process = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(process) => process,
            Err(error) if error.code().0 as u32 == 0x80070057 => return Some(false),
            Err(_) => return None,
        };
        let result = (|| {
            let mut exit = 0;
            GetExitCodeProcess(process, &mut exit).ok()?;
            if exit != 259 {
                return Some(false);
            }
            let mut created = FILETIME::default();
            let mut exited = FILETIME::default();
            let mut kernel = FILETIME::default();
            let mut user = FILETIME::default();
            GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user).ok()?;
            let actual = ((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64;
            if actual != expected {
                return Some(false);
            }
            let mut buffer = [0u16; 32768];
            let mut length = buffer.len() as u32;
            QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                PWSTR(buffer.as_mut_ptr()),
                &mut length,
            )
            .ok()?;
            let path = String::from_utf16_lossy(&buffer[..length as usize]);
            let exe = Path::new(&path).file_name()?.to_str()?;
            Some(exe.eq_ignore_ascii_case("claude.exe") || exe.eq_ignore_ascii_case("node.exe"))
        })();
        let _ = CloseHandle(process);
        result
    }
}

#[cfg(not(windows))]
fn claude_process_matches(_pid: u32, _start: Option<&str>) -> Option<bool> {
    None // Do not infer liveness from PID existence or a timestamp on another platform.
}
