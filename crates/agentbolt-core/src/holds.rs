//! Advisory path holds. The daemon scans only Desk-configured working copies; agents can manage only
//! their own temporary holds. A missing, stale or failed scan is unknown, never proof of a clean copy.
use crate::{
    board, events,
    model::Agent,
    store::{self, db_err, fmt_ms},
    tokens,
};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const FRESH_MS: i64 = 180_000;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub working_copies: Vec<WorkingCopy>,
    #[serde(default)]
    pub excluded_paths: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkingCopy {
    pub path: String,
    pub repo: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Scan {
    pub path: String,
    pub repo: String,
    pub at_ms: i64,
    pub paths: Vec<String>,
    pub error: Option<String>,
    #[serde(default)]
    pub conservative_submodules: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hold {
    pub id: String,
    pub owner: String,
    pub kind: String,
    pub repo: String,
    pub paths: Vec<String>,
    pub note: String,
    pub until: Option<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Snapshot {
    pub config: Config,
    pub scans: Vec<Scan>,
    pub holds: Vec<Hold>,
    #[serde(default)]
    pub next: Option<String>,
    #[serde(default)]
    pub more: bool,
    #[serde(default)]
    pub total_agent_holds: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Check {
    pub repo: String,
    pub paths: Vec<String>,
    pub clear: bool,
    pub collisions: Vec<Hold>,
    pub unknown: Vec<String>,
    #[serde(default)]
    pub total_collisions: u32,
    #[serde(default)]
    pub more_collisions: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum HoldOp {
    List,
    ListPage {
        after: Option<String>,
        repo: Option<String>,
    },
    Check {
        repo: String,
        paths: Vec<String>,
        #[serde(default)]
        stage: Option<String>,
    },
    Acquire {
        repo: String,
        paths: Vec<String>,
        note: String,
        #[serde(default)]
        id: Option<String>,
    },
    Release {
        id: String,
    },
}

/// Lexical validation runs before any filesystem access, so excluded roots are not opened to resolve them.
pub fn path_key(path: &str) -> Result<String, String> {
    let path = path.trim().replace('\\', "/");
    if path.len() > 2048
        || path.chars().any(char::is_control)
        || path.starts_with("//")
        || path.split('/').any(|p| matches!(p, "." | ".."))
    {
        return Err(
            "working-copy paths must be absolute local paths without '..' or control characters"
                .into(),
        );
    }
    let absolute = path.starts_with('/')
        || (path.as_bytes().get(1) == Some(&b':')
            && path.as_bytes().get(2) == Some(&b'/')
            && path.as_bytes()[0].is_ascii_alphabetic());
    if !absolute || path.trim_end_matches('/').len() < 3 {
        return Err("choose a working-copy directory, not a filesystem root".into());
    }
    Ok(path.trim_end_matches('/').to_lowercase())
}
pub fn excluded(path: &str, config: &Config) -> bool {
    let Ok(path) = path_key(path) else {
        return true;
    };
    config.excluded_paths.iter().any(|root| {
        path_key(root).is_ok_and(|r| {
            path == r
                || path.starts_with(&(r.clone() + "/"))
                || r.starts_with(&(path.clone() + "/"))
        })
    })
}
pub fn config(db: &Connection) -> Result<Config, String> {
    store::meta(db, "holds_config")
        .map_err(db_err)?
        .map(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
        .unwrap_or_else(|| Ok(Config::default()))
}
pub fn set_config(db: &Connection, now: i64, mut cfg: Config) -> Result<Config, String> {
    if cfg.working_copies.len() > 100 || cfg.excluded_paths.len() > 100 {
        return Err("at most 100 working copies and 100 excluded roots".into());
    }
    for path in &cfg.excluded_paths {
        path_key(path)?;
    }
    let mut seen = std::collections::HashSet::new();
    for source in &mut cfg.working_copies {
        source.path = source.path.trim().to_owned();
        let key = path_key(&source.path)?;
        if !seen.insert(key) {
            return Err("duplicate working-copy path".into());
        }
        source.repo = board::repository(&source.repo)?;
    }
    if cfg.working_copies.iter().any(|s| excluded(&s.path, &cfg)) {
        return Err(
            "an allowlisted working copy overlaps an excluded root; it cannot be scanned".into(),
        );
    }
    // Removing a source removes its saved path data too; a later scan cannot reinsert it (save_scan rechecks).
    db.execute("DELETE FROM hold_scans", []).map_err(db_err)?;
    store::set_meta(db, "holds_config", &json!(cfg).to_string()).map_err(db_err)?;
    events::append(
        db,
        now,
        "holds.configured",
        "holds",
        &json!({"sources":cfg.working_copies.len(),"excluded_roots":cfg.excluded_paths.len()}),
    )
    .map_err(db_err)?;
    Ok(cfg)
}
pub fn save_scan(db: &Connection, scan: Scan) -> Result<(), String> {
    let cfg = config(db)?;
    if !cfg
        .working_copies
        .iter()
        .any(|s| s.path == scan.path && s.repo == scan.repo)
        || excluded(&scan.path, &cfg)
    {
        return Ok(());
    }
    db.execute("INSERT INTO hold_scans(source,result_json) VALUES (?1,?2) ON CONFLICT(source) DO UPDATE SET result_json=excluded.result_json",params![path_key(&scan.path)?,json!(scan).to_string()]).map_err(db_err)?;
    Ok(())
}
fn working_copies(db: &Connection) -> Result<Snapshot, String> {
    let cfg = config(db)?;
    let mut stmt = db
        .prepare("SELECT result_json FROM hold_scans")
        .map_err(db_err)?;
    let scans = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(db_err)?
        .map(|r| serde_json::from_str::<Scan>(&r.map_err(db_err)?).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, String>>()?;
    let mut holds = Vec::new();
    for scan in &scans {
        if !scan.paths.is_empty() {
            holds.push(Hold {
                id: format!("copy:{}", scan.path),
                owner: "Artem".into(),
                kind: "working_copy".into(),
                repo: scan.repo.clone(),
                paths: scan.paths.clone(),
                note: scan
                    .error
                    .clone()
                    .unwrap_or_else(|| if scan.conservative_submodules {
                        "Dirty/unpushed paths and conservative submodule holds; nested checkouts are not scanned".into()
                    } else { "Dirty or unpushed paths".into() }),
                until: None,
            });
        }
    }
    Ok(Snapshot {
        config: cfg,
        scans,
        holds,
        ..Default::default()
    })
}

fn agent_hold(row: &rusqlite::Row<'_>) -> rusqlite::Result<Hold> {
    let raw: String = row.get(3)?;
    let paths = serde_json::from_str(&raw).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e))
    })?;
    Ok(Hold {
        id: row.get(0)?,
        owner: row.get(1)?,
        repo: row.get(2)?,
        paths,
        note: row.get(4)?,
        until: Some(fmt_ms(row.get(5)?)),
        kind: "agent".into(),
    })
}
pub fn snapshot(db: &Connection, now: i64) -> Result<Snapshot, String> {
    snapshot_page(db, now, None, None)
}
pub fn snapshot_page(
    db: &Connection,
    now: i64,
    after: Option<String>,
    repo: Option<String>,
) -> Result<Snapshot, String> {
    let repo = repo.map(|r| board::repository(&r)).transpose()?;
    let mut state = working_copies(db)?;
    if let Some(repo) = &repo {
        state.holds.retain(|h| &h.repo == repo);
    }
    state.total_agent_holds = store::count(
        db,
        "SELECT COUNT(*) FROM agent_holds WHERE expires_ms>?1 AND (?2 IS NULL OR repo=?2)",
        params![now, repo],
    )
    .map_err(db_err)?;
    let mut stmt=db.prepare("SELECT id,owner,repo,paths_json,note,expires_ms FROM agent_holds WHERE expires_ms>?1 AND (?2 IS NULL OR repo=?2) AND (?3 IS NULL OR id>?3) ORDER BY id LIMIT 51").map_err(db_err)?;
    let mut page = stmt
        .query_map(params![now, repo, after], agent_hold)
        .map_err(db_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_err)?;
    state.more = page.len() > 50;
    page.truncate(50);
    state.next = page.last().map(|h| h.id.clone());
    state.holds.extend(page);
    Ok(state)
}

/// '*' spans one path component; '**' spans components; '?' matches one non-separator character.
pub fn matches(pattern: &str, path: &str) -> bool {
    if !pattern.contains(['*', '?']) {
        return pattern == path;
    }
    #[derive(Clone, Copy)]
    enum State {
        Literal(char),
        Any,
        Star,
        DeepStar,
        DirectoryStart,
        DirectoryRest,
    }
    // A small forward automaton keeps stack and memory independent of path length.
    // **/ can consume zero directories or finish at a slash, never part of a filename.
    let mut states = Vec::new();
    let mut chars = pattern.chars().peekable();
    while let Some(ch) = chars.next() {
        let state = match ch {
            '*' if chars.peek() == Some(&'*') => {
                chars.next();
                if chars.peek() == Some(&'/') {
                    chars.next();
                    states.push(State::DirectoryStart);
                    State::DirectoryRest
                } else {
                    State::DeepStar
                }
            }
            '*' => State::Star,
            '?' => State::Any,
            ch => State::Literal(ch),
        };
        states.push(state);
    }
    let closure = |active: &mut [bool]| {
        // Epsilon edges only go forward, so one pass reaches the full closure.
        for (i, state) in states.iter().enumerate() {
            if active[i] {
                match state {
                    State::Star | State::DeepStar => active[i + 1] = true,
                    State::DirectoryStart => active[i + 2] = true,
                    _ => {}
                }
            }
        }
    };
    let mut active = vec![false; states.len() + 1];
    let mut next = active.clone();
    active[0] = true;
    closure(&mut active);
    for ch in path.chars() {
        next.fill(false);
        for (i, state) in states.iter().enumerate() {
            if !active[i] {
                continue;
            }
            match state {
                State::Literal(value) if *value == ch => next[i + 1] = true,
                State::Any if ch != '/' => next[i + 1] = true,
                State::Star if ch != '/' => next[i] = true,
                State::DeepStar => next[i] = true,
                State::DirectoryStart => {
                    next[i + 1] = true;
                    if ch == '/' {
                        next[i + 2] = true;
                    }
                }
                State::DirectoryRest => {
                    next[i] = true;
                    if ch == '/' {
                        next[i + 1] = true;
                    }
                }
                _ => {}
            }
        }
        closure(&mut next);
        std::mem::swap(&mut active, &mut next);
        if !active.iter().any(|value| *value) {
            return false;
        }
    }
    active[states.len()]
}
fn overlap(a: &str, b: &str) -> bool {
    if !a.contains(['*', '?']) {
        return matches(b, a);
    }
    if !b.contains(['*', '?']) {
        return matches(a, b);
    }
    // Pattern-to-pattern overlap errs toward flagging shared literal prefixes, never saying clear.
    let a = a.split(['*', '?']).next().unwrap_or("");
    let b = b.split(['*', '?']).next().unwrap_or("");
    a.starts_with(b) || b.starts_with(a)
}
pub fn check(
    db: &Connection,
    now: i64,
    repo: &str,
    requested: Vec<String>,
    ignore: Option<&str>,
) -> Result<Check, String> {
    let repo = board::repository(repo)?;
    let requested = board::paths(requested)?;
    if requested.is_empty() {
        return Err("provide paths to check, or '**' for the whole repository".into());
    }
    let state = working_copies(db)?;
    let mut unknown = Vec::new();
    let sources: Vec<_> = state
        .config
        .working_copies
        .iter()
        .filter(|s| s.repo == repo)
        .collect();
    if sources.is_empty() {
        unknown.push("No working copy is configured for this repository".into());
    }
    for source in sources {
        if excluded(&source.path, &state.config) {
            unknown.push(format!(
                "{} overlaps an excluded root and cannot be scanned",
                source.path
            ));
            continue;
        }
        match state
            .scans
            .iter()
            .find(|s| s.path == source.path && s.repo == repo)
        {
            None => unknown.push(format!("{} has not been scanned", source.path)),
            Some(s) => {
                if let Some(error) = &s.error {
                    unknown.push(format!("{}: {error}", s.path));
                }
                if s.at_ms > now {
                    unknown.push(format!("{} scan time is in the future", s.path));
                }
                if now.saturating_sub(s.at_ms) > FRESH_MS {
                    unknown.push(format!("{} scan is stale", s.path));
                }
            }
        }
    }
    let collides = |h: &Hold| {
        h.repo == repo
            && ignore != Some(h.id.as_str())
            && h.paths
                .iter()
                .any(|p| requested.iter().any(|q| overlap(p, q)))
    };
    let mut collisions = Vec::new();
    let mut total_collisions = 0u32;
    let mut record = |hold: Hold| {
        if collides(&hold) {
            total_collisions = total_collisions.saturating_add(1);
            if collisions.len() < 50 {
                collisions.push(hold);
            }
        }
    };
    for hold in state.holds {
        record(hold);
    }
    // Stream all relevant agent holds. Display pagination must never become a false clearance.
    let mut stmt=db.prepare("SELECT id,owner,repo,paths_json,note,expires_ms FROM agent_holds WHERE expires_ms>?1 AND repo=?2 ORDER BY id").map_err(db_err)?;
    for row in stmt
        .query_map(params![now, repo], agent_hold)
        .map_err(db_err)?
    {
        record(row.map_err(db_err)?);
    }
    Ok(Check {
        repo,
        paths: requested,
        clear: total_collisions == 0 && unknown.is_empty(),
        more_collisions: total_collisions > collisions.len() as u32,
        total_collisions,
        collisions,
        unknown,
    })
}
pub fn touch(db: &Connection, now: i64, handle: &str) -> Result<(), String> {
    db.execute("UPDATE agent_holds SET expires_ms=?1+1000*COALESCE((SELECT l.lease_seconds FROM board_limits l JOIN agents a ON a.owner=l.owner WHERE a.handle=?2),14400) WHERE handle=?2 AND expires_ms>?1",params![now,handle]).map_err(db_err)?;
    Ok(())
}
pub fn run(db: &Connection, now: i64, agent: &Agent, action: HoldOp) -> Result<Value, String> {
    match action {
        HoldOp::List => Ok(json!(snapshot(db, now)?)),
        HoldOp::ListPage { after, repo } => Ok(json!(snapshot_page(db, now, after, repo)?)),
        HoldOp::Check { repo, paths, stage } => {
            let result = check(db, now, &repo, paths, None)?;
            if let Some(stage) = stage {
                if !matches!(stage.as_str(), "pr_open" | "merge") {
                    return Err("stage must be pr_open or merge".into());
                }
                events::append(db,now,"holds.checked",&result.repo,&json!({"stage":stage,"owner":agent.owner,"collisions":result.total_collisions,"unknown":result.unknown.len(),"clear":result.clear})).map_err(db_err)?;
            }
            Ok(json!(result))
        }
        HoldOp::Acquire {
            repo,
            paths,
            note,
            id,
        } => {
            let repo = board::repository(&repo)?;
            let paths = board::paths(paths)?;
            if paths.is_empty() || note.trim().is_empty() || note.len() > 2000 {
                return Err("a hold needs paths and a short reason (1–2000 bytes)".into());
            }
            let count: u32 = db
                .query_row(
                    "SELECT COUNT(*) FROM agent_holds WHERE owner=?1 AND expires_ms>?2",
                    params![agent.owner, now],
                    |r| r.get(0),
                )
                .map_err(db_err)?;
            if let Some(id) = &id {
                let owns:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM agent_holds WHERE id=?1 AND handle=?2 AND expires_ms>?3)",params![id,agent.handle,now],|r|r.get(0)).map_err(db_err)?;
                if !owns {
                    return Err("unknown, expired or another agent's hold".into());
                }
            } else if count >= 10 {
                return Err("this owner already has 10 active holds".into());
            }
            let collision = check(db, now, &repo, paths.clone(), id.as_deref())?;
            let id = id.unwrap_or_else(|| format!("hold_{}", tokens::random_hex(12)));
            let until = now + i64::from(board::lease_seconds(db, &agent.owner)?) * 1000;
            db.execute("INSERT INTO agent_holds(id,owner,handle,repo,paths_json,note,expires_ms) VALUES (?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(id) DO UPDATE SET repo=excluded.repo,paths_json=excluded.paths_json,note=excluded.note,expires_ms=excluded.expires_ms",params![id,agent.owner,agent.handle,repo,json!(paths).to_string(),note.trim(),until]).map_err(db_err)?;
            events::append(
                db,
                now,
                "holds.acquired",
                &id,
                &json!({"owner":agent.owner,"repo":repo,"paths":paths}),
            )
            .map_err(db_err)?;
            Ok(
                json!({"id":id,"until":fmt_ms(until),"overlap":collision,"notice":"Advisory hold only; overlapping work is not authorized by acquiring it."}),
            )
        }
        HoldOp::Release { id } => {
            let changed = db
                .execute(
                    "DELETE FROM agent_holds WHERE id=?1 AND handle=?2",
                    params![id, agent.handle],
                )
                .map_err(db_err)?;
            if changed == 0 {
                return Err("unknown or another agent's hold".into());
            }
            events::append(
                db,
                now,
                "holds.released",
                &id,
                &json!({"owner":agent.owner}),
            )
            .map_err(db_err)?;
            Ok(json!({"released":id}))
        }
    }
}

/// Import only eligible project entries, without opening their folders. The Desk shows the result for review.
pub fn from_focus(value: Value, excluded_paths: Vec<String>) -> Result<Config, String> {
    let projects = value
        .get("projects")
        .and_then(Value::as_array)
        .ok_or("FOCUS.json needs a projects array")?;
    let mut working_copies = Vec::new();
    for p in projects {
        if !matches!(
            p.get("verdict").and_then(Value::as_str),
            Some("invest" | "maintain")
        ) {
            continue;
        }
        if let (Some(path), Some(repo)) = (
            p.get("path").and_then(Value::as_str),
            p.get("repo").and_then(Value::as_str),
        ) {
            path_key(path)?;
            working_copies.push(WorkingCopy {
                path: path.into(),
                repo: board::repository(repo)?,
            });
        }
    }
    let mut config = Config {
        working_copies,
        excluded_paths,
    };
    let excludes = Config {
        working_copies: Vec::new(),
        excluded_paths: config.excluded_paths.clone(),
    };
    config
        .working_copies
        .retain(|s| !excluded(&s.path, &excludes));
    Ok(config)
}
