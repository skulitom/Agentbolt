//! Bounded, incremental, in-memory search of visible local conversation messages.
//! Never indexes reasoning, tool payloads, credentials, attachments or arbitrary project files.

use crate::session_search::{Hit, parse};
use crate::sessions::{Activity, Roots, Session, clean_title, local_path};
use agentbolt_core::navigation::{ChatTarget, valid_session_id};
use rusqlite::{Connection, OpenFlags, params};
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};

const CHAT_BYTES: usize = 512 * 1024;
const TOTAL_BYTES: usize = 64 * 1024 * 1024;
const FILE_BYTES: u64 = 8 * 1024 * 1024;
const FILE_LIMIT: usize = 5_000;

struct Source {
    key: String,
    stamp: String,
    path: PathBuf,
    session: Session,
    codex: bool,
}

struct Record {
    stamp: String,
    session: Session,
    bytes: usize,
    limited: bool,
}

pub struct Library {
    db: Connection,
    records: HashMap<String, Record>,
    queue: VecDeque<Source>,
    inventoried: Option<Instant>,
    notes: Vec<String>,
    used: usize,
    total: usize,
}

pub fn key(item: &Session) -> String {
    format!(
        "{}:{}",
        if matches!(item.target, ChatTarget::Codex(_)) {
            "codex"
        } else {
            "claude"
        },
        item.source_id
    )
}

impl Library {
    pub fn new() -> Result<Self, String> {
        let db = Connection::open_in_memory().map_err(|e| e.to_string())?;
        db.execute_batch("PRAGMA temp_store=MEMORY; CREATE VIRTUAL TABLE messages USING fts5(body, context, session UNINDEXED, tokenize='unicode61');")
            .map_err(|e| e.to_string())?;
        Ok(Self {
            db,
            records: HashMap::new(),
            queue: VecDeque::new(),
            inventoried: None,
            notes: Vec::new(),
            used: 0,
            total: 0,
        })
    }

    pub fn sync(&mut self, roots: &Roots, items: &[Session]) -> Result<(), String> {
        // An indexing batch always finishes before replacing its inventory. Live messages update
        // on the next inventory, at most every 15 seconds while someone is searching.
        if !self.queue.is_empty()
            || self
                .inventoried
                .is_some_and(|t| t.elapsed() < Duration::from_secs(15))
        {
            return Ok(());
        }
        self.notes.clear();
        let mut sources = Vec::new();
        if let Some(path) = newest_history(&roots.codex) {
            for item in items
                .iter()
                .filter(|s| matches!(s.target, ChatTarget::Codex(_)))
            {
                sources.push(Source {
                    key: key(item),
                    stamp: format!(
                        "{}:{}:{}:{}",
                        path.display(),
                        item.updated,
                        item.title,
                        if item.running() {
                            time::OffsetDateTime::now_utc().unix_timestamp() / 15
                        } else {
                            0
                        }
                    ),
                    path: path.clone(),
                    session: item.clone(),
                    codex: true,
                });
            }
        } else if items
            .iter()
            .any(|s| matches!(s.target, ChatTarget::Codex(_)))
        {
            self.notes.push(
                "Codex message history is unavailable. Title and project search still works."
                    .into(),
            );
        }
        let known: HashMap<_, _> = items
            .iter()
            .filter(|s| !matches!(s.target, ChatTarget::Codex(_)))
            .map(|s| (s.source_id.as_str(), s))
            .collect();
        if let Some(projects) = local_path(&roots.claude.join("projects")) {
            let mut files = Vec::new();
            let mut entries_seen = 0;
            'folders: for folder in fs::read_dir(projects)
                .map_err(|e| e.to_string())?
                .take(FILE_LIMIT)
            {
                let folder = folder.map_err(|e| e.to_string())?;
                let Some(folder) = local_path(&folder.path()).filter(|p| p.is_dir()) else {
                    continue;
                };
                for file in fs::read_dir(folder).map_err(|e| e.to_string())? {
                    entries_seen += 1;
                    if entries_seen > 20_000 {
                        self.notes.push(
                            "Some Claude history is outside the 20,000-entry folder scan limit."
                                .into(),
                        );
                        break 'folders;
                    }
                    let file = file.map_err(|e| e.to_string())?;
                    let path = file.path();
                    if path.extension().and_then(|x| x.to_str()) != Some("jsonl") {
                        continue;
                    }
                    let Some(id) = path
                        .file_stem()
                        .and_then(|x| x.to_str())
                        .filter(|s| valid_session_id(s))
                    else {
                        continue;
                    };
                    let id = id.to_owned();
                    let Some(path) = local_path(&path) else {
                        continue;
                    };
                    let metadata = fs::metadata(&path).map_err(|e| e.to_string())?;
                    if !metadata.is_file() {
                        continue;
                    }
                    let modified = metadata
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .unwrap_or_default();
                    files.push((modified, path, id, metadata.len()));
                    if files.len() >= FILE_LIMIT {
                        break;
                    }
                }
                if files.len() >= FILE_LIMIT {
                    break;
                }
            }
            if files.len() >= FILE_LIMIT {
                self.notes.push(
                    "Message search is limited to 5,000 local Claude transcript files.".into(),
                );
            }
            files.sort_by(|a, b| b.0.cmp(&a.0));
            let mut seen = HashSet::new();
            for (modified, path, id, len) in files {
                if !seen.insert(id.clone()) {
                    continue;
                }
                let session = known
                    .get(id.as_str())
                    .map(|s| (*s).clone())
                    .unwrap_or_else(|| Session {
                        target: ChatTarget::ClaudeTerminal(id.clone()),
                        source_id: id,
                        title: "Claude chat".into(),
                        cwd: String::new(),
                        updated: modified.as_secs() as i64,
                        open: false,
                        background: false,
                        activity: Activity::Unknown,
                    });
                sources.push(Source {
                    key: key(&session),
                    stamp: format!(
                        "{}:{len}:{}:{}",
                        path.display(),
                        modified.as_nanos(),
                        session.title
                    ),
                    path,
                    session,
                    codex: false,
                });
            }
        } else if !known.is_empty() {
            self.notes.push(
                "Claude message history is unavailable. Title and project search still works."
                    .into(),
            );
        }
        sources.sort_by(|a, b| b.session.updated.cmp(&a.session.updated));
        self.total = sources.len();
        let wanted: HashSet<_> = sources.iter().map(|s| s.key.clone()).collect();
        let stale: Vec<_> = self
            .records
            .keys()
            .filter(|key| !wanted.contains(*key))
            .cloned()
            .collect();
        for key in stale {
            self.remove(&key)?;
        }
        for source in sources {
            if self
                .records
                .get(&source.key)
                .is_some_and(|r| r.stamp == source.stamp)
            {
                continue;
            }
            self.remove(&source.key)?;
            self.queue.push_back(source);
        }
        self.inventoried = Some(Instant::now());
        Ok(())
    }

    fn remove(&mut self, key: &str) -> Result<(), String> {
        if let Some(record) = self.records.remove(key) {
            self.used = self.used.saturating_sub(record.bytes);
            self.db
                .execute("DELETE FROM messages WHERE session=?1", [key])
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn advance(&mut self) -> Result<(), String> {
        let started = Instant::now();
        while started.elapsed() < Duration::from_millis(150) {
            let Some(source) = self.queue.pop_front() else {
                break;
            };
            let (mut body, session, mut limited) = match read_source(&source) {
                Ok(result) => result,
                Err(_) => {
                    self.notes.push("Some local messages could not be read. Those chats remain searchable by title and project.".into());
                    (String::new(), source.session.clone(), true)
                }
            };
            if self.used + body.len() > TOTAL_BYTES {
                body.clear();
                limited = true;
            }
            let context = format!(
                "{} {} {} {}",
                session.title,
                session.cwd,
                session.app(),
                session.target.id()
            );
            self.db
                .execute(
                    "INSERT INTO messages(body,context,session) VALUES (?1,?2,?3)",
                    params![body, context, source.key],
                )
                .map_err(|e| e.to_string())?;
            self.used += body.len();
            self.records.insert(
                source.key,
                Record {
                    stamp: source.stamp,
                    session,
                    bytes: body.len(),
                    limited,
                },
            );
        }
        Ok(())
    }

    pub fn enrich(&self, items: &mut Vec<Session>) {
        let mut known: HashSet<_> = items.iter().map(key).collect();
        let mut extra: Vec<_> = self
            .records
            .iter()
            .filter(|(key, _)| known.insert((*key).clone()))
            .filter(|(_, r)| !matches!(r.session.target, ChatTarget::Codex(_)))
            .map(|(_, r)| r.session.clone())
            .collect();
        extra.sort_by(|a, b| {
            b.updated
                .cmp(&a.updated)
                .then(a.source_id.cmp(&b.source_id))
        });
        items.extend(extra);
    }

    pub fn merge(&self, query: &str, items: &[Session], hits: &mut Vec<Hit>) -> Result<(), String> {
        let terms = parse(query);
        let literal = |term: &crate::session_search::Term| {
            format!(
                "\"{}\"{}",
                term.text.replace('"', "\"\""),
                if term.phrase { "" } else { "*" }
            )
        };
        let positive: Vec<_> = terms.iter().filter(|t| !t.exclude).map(literal).collect();
        let negative: Vec<_> = terms.iter().filter(|t| t.exclude).map(literal).collect();
        if !negative.is_empty() {
            let mut stmt = self
                .db
                .prepare("SELECT session FROM messages WHERE messages MATCH ?1")
                .map_err(|e| e.to_string())?;
            let excluded: HashSet<String> = stmt
                .query_map([negative.join(" OR ")], |r| r.get(0))
                .map_err(|e| e.to_string())?
                .collect::<rusqlite::Result<_>>()
                .map_err(|e| e.to_string())?;
            hits.retain(|h| !excluded.contains(&key(&items[h.item])));
        }
        if positive.is_empty() {
            return Ok(());
        }
        let mut expression = format!(
            "({}) AND body : ({})",
            positive.join(" AND "),
            positive.join(" OR ")
        );
        if !negative.is_empty() {
            expression = format!("({expression}) NOT ({})", negative.join(" OR "));
        }
        let positions: HashMap<_, _> = items.iter().enumerate().map(|(i, s)| (key(s), i)).collect();
        let mut seen: HashSet<_> = hits.iter().map(|h| h.item).collect();
        let mut stmt = self.db.prepare("SELECT session, snippet(messages, 0, '', '', ' … ', 28) FROM messages WHERE messages MATCH ?1 ORDER BY bm25(messages, 1.0, 0.3) LIMIT 10000")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([expression], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?;
        for row in rows {
            let (key, snippet) = row.map_err(|e| e.to_string())?;
            let Some(&item) = positions.get(&key) else {
                continue;
            };
            if seen.insert(item) {
                hits.push(Hit {
                    item,
                    score: 10,
                    label: "Message match",
                    approximate: false,
                    snippet: Some(
                        snippet
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" ")
                            .chars()
                            .take(300)
                            .collect(),
                    ),
                });
            }
        }
        // Stable sort preserves FTS relevance among message matches.
        hits.sort_by(|a, b| b.score.cmp(&a.score));
        Ok(())
    }

    pub fn progress(&self) -> (usize, usize) {
        (self.total.saturating_sub(self.queue.len()), self.total)
    }
    pub fn pending(&self) -> bool {
        !self.queue.is_empty()
    }
    pub fn notes(&self) -> Vec<String> {
        let mut notes = self.notes.clone();
        if self.records.values().any(|r| r.limited) {
            notes.push("Some message history is outside the search limits or unavailable. Search covers up to 512 KiB of recent message text per chat, 1,000 Codex messages, the last 8 MiB of each Claude transcript, and 64 MiB of message text in total.".into());
        }
        notes.sort();
        notes.dedup();
        notes
    }
}

fn newest_history(root: &Path) -> Option<PathBuf> {
    let root = local_path(root)?;
    fs::read_dir(root)
        .ok()?
        .take(FILE_LIMIT)
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let version = name
                .strip_prefix("thread_history_")?
                .strip_suffix(".sqlite")?
                .parse::<u32>()
                .ok()?;
            Some((version, entry.path()))
        })
        .max_by_key(|(version, _)| *version)
        .and_then(|(_, path)| local_path(&path))
}

fn append(body: &mut String, text: &str) -> bool {
    let available = CHAT_BYTES.saturating_sub(body.len() + 1);
    let mut end = text.len().min(available);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    body.push_str(&text[..end]);
    if body.len() < CHAT_BYTES {
        body.push('\n');
    }
    end < text.len()
}

fn text_blocks(value: &Value) -> Vec<&str> {
    if let Some(text) = value.as_str() {
        return vec![text];
    }
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .collect()
}

fn read_source(source: &Source) -> Result<(String, Session, bool), String> {
    let path = local_path(&source.path).ok_or("Message source is unavailable")?;
    let mut session = source.session.clone();
    let mut body = String::new();
    let mut limited = false;
    if source.codex {
        let db = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| e.to_string())?;
        db.busy_timeout(Duration::from_millis(100))
            .map_err(|e| e.to_string())?;
        db.pragma_update(None, "query_only", true)
            .map_err(|e| e.to_string())?;
        let mut stmt = db.prepare("SELECT substr(item_json,1,1048576) FROM thread_items WHERE thread_id=?1 AND item_type IN ('userMessage','agentMessage') ORDER BY rollout_ordinal DESC LIMIT 1001").map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([&session.source_id], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        for (i, row) in rows.enumerate() {
            if i >= 1000 || body.len() >= CHAT_BYTES {
                limited = true;
                break;
            }
            let raw = row.map_err(|e| e.to_string())?;
            let Ok(item) = serde_json::from_str::<Value>(&raw) else {
                limited = true;
                continue;
            };
            match item.get("type").and_then(Value::as_str) {
                Some("agentMessage") => {
                    if let Some(text) = item.get("text").and_then(Value::as_str) {
                        limited |= append(&mut body, text);
                    }
                }
                Some("userMessage") => {
                    for text in text_blocks(&item["content"]) {
                        limited |= append(&mut body, text);
                    }
                }
                _ => {}
            }
        }
    } else {
        let mut file = File::open(path).map_err(|e| e.to_string())?;
        let len = file.metadata().map_err(|e| e.to_string())?.len();
        let start = len.saturating_sub(FILE_BYTES);
        file.seek(SeekFrom::Start(start))
            .map_err(|e| e.to_string())?;
        let mut reader = BufReader::new(file.take(FILE_BYTES));
        let mut line = Vec::new();
        if start > 0 {
            reader
                .read_until(b'\n', &mut line)
                .map_err(|e| e.to_string())?;
            limited = true;
        }
        let mut messages = VecDeque::<String>::new();
        let mut bytes = 0;
        let mut fallback_title = None;
        loop {
            line.clear();
            if reader
                .read_until(b'\n', &mut line)
                .map_err(|e| e.to_string())?
                == 0
            {
                break;
            }
            let Ok(item) = serde_json::from_slice::<Value>(&line) else {
                limited = true;
                continue;
            };
            if item
                .get("sessionId")
                .and_then(Value::as_str)
                .is_some_and(|id| id != session.source_id)
            {
                continue;
            }
            if session.cwd.is_empty()
                && let Some(cwd) = item.get("cwd").and_then(Value::as_str)
            {
                session.cwd = cwd.chars().take(4096).collect();
            }
            let kind = item.get("type").and_then(Value::as_str);
            if kind == Some("custom-title")
                && source.session.title == "Claude chat"
                && let Some(title) = item.get("customTitle").and_then(Value::as_str)
            {
                session.title = clean_title(title, "Claude chat");
            }
            if !matches!(kind, Some("user" | "assistant"))
                || item.get("isMeta").and_then(Value::as_bool) == Some(true)
            {
                continue;
            }
            session.background |= item.get("isSidechain").and_then(Value::as_bool) == Some(true);
            let mut message = String::new();
            for text in text_blocks(&item["message"]["content"]) {
                limited |= append(&mut message, text);
            }
            if kind == Some("user") && fallback_title.is_none() && !message.trim().is_empty() {
                fallback_title = Some(clean_title(&message, "Claude chat"));
            }
            if message.trim().is_empty() {
                continue;
            }
            bytes += message.len();
            messages.push_back(message);
            while bytes > CHAT_BYTES {
                if let Some(old) = messages.pop_front() {
                    bytes -= old.len();
                    limited = true;
                }
            }
        }
        if session.title == "Claude chat"
            && let Some(title) = fallback_title
        {
            session.title = title;
        }
        for message in messages {
            limited |= append(&mut body, &message);
        }
    }
    Ok((body, session, limited))
}
