//! The SQLite store: connection setup, schema and migrations, and row mapping.
//!
//! The daemon is the only writer: the file is opened with `PRAGMA locking_mode=EXCLUSIVE` and WAL, and the
//! exclusive lock is taken at open, so a second daemon (or any other process) can't open the same database.
//! Functions here take a `&Connection` so callers can run several of them inside one transaction. The rules
//! (budget, owners, dedupe, defaults) live in `rules` and `service_impl`.

use std::path::Path;
use std::time::Duration;

use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, Params, Row, params};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::model::{Agent, Answer, AnswerVia, AskInput, AskStatus, Delivery, Owner, Settings};
use crate::rules;

/// Each entry upgrades the schema by one version (`PRAGMA user_version`). Never edit a released entry; append.
const MIGRATIONS: &[&str] = &[
    r#"
CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) WITHOUT ROWID;

CREATE TABLE owners (
    name              TEXT PRIMARY KEY,
    binding           TEXT NOT NULL,
    codex_interactive INTEGER NOT NULL DEFAULT 0,
    relay             INTEGER NOT NULL DEFAULT 0,
    created_ms        INTEGER NOT NULL
);

CREATE TABLE agents (
    handle        TEXT PRIMARY KEY,
    owner         TEXT NOT NULL,
    client        TEXT NOT NULL,
    session_ref   TEXT,
    cwd           TEXT,
    machine       TEXT NOT NULL,
    doing         TEXT,
    registered_ms INTEGER NOT NULL,
    last_seen_ms  INTEGER NOT NULL,
    connection    INTEGER
);
CREATE INDEX agents_owner ON agents(owner);

CREATE TABLE asks (
    id             TEXT PRIMARY KEY,
    owner          TEXT NOT NULL,
    asking_session TEXT,
    input_json     TEXT NOT NULL,
    priority       TEXT NOT NULL,
    p0_skip        INTEGER NOT NULL DEFAULT 0,
    reversible     INTEGER NOT NULL,
    deadline_ms    INTEGER NOT NULL,
    status         TEXT NOT NULL,
    created_ms     INTEGER NOT NULL,
    inbox_ms       INTEGER,
    shown_ms       INTEGER,
    closed_ms      INTEGER
);
CREATE INDEX asks_status ON asks(status, deadline_ms);
CREATE INDEX asks_owner ON asks(owner, status);
CREATE INDEX asks_inbox ON asks(inbox_ms);

CREATE TABLE answers (
    ask_id      TEXT PRIMARY KEY REFERENCES asks(id),
    via         TEXT NOT NULL,
    choice      TEXT,
    values_json TEXT,
    note        TEXT,
    answered_ms INTEGER NOT NULL
);

CREATE TABLE deliveries (
    id     INTEGER PRIMARY KEY AUTOINCREMENT,
    ask_id TEXT NOT NULL REFERENCES asks(id),
    target TEXT NOT NULL,
    at_ms  INTEGER NOT NULL,
    ok     INTEGER NOT NULL,
    detail TEXT
);
CREATE INDEX deliveries_ask ON deliveries(ask_id);

CREATE TABLE toasts (
    toast_id    TEXT PRIMARY KEY,
    ask_id      TEXT NOT NULL REFERENCES asks(id),
    created_ms  INTEGER NOT NULL,
    expires_ms  INTEGER NOT NULL,
    used_ms     INTEGER,
    used_option INTEGER,
    revoked_ms  INTEGER
);
CREATE INDEX toasts_ask ON toasts(ask_id);

CREATE TABLE events (
    seq       INTEGER PRIMARY KEY,
    at        TEXT NOT NULL,
    kind      TEXT NOT NULL,
    subject   TEXT NOT NULL,
    data      TEXT NOT NULL,
    prev_hash TEXT NOT NULL,
    hash      TEXT NOT NULL
);
CREATE TRIGGER events_no_update BEFORE UPDATE ON events
BEGIN SELECT RAISE(ABORT, 'events are append-only'); END;
CREATE TRIGGER events_no_delete BEFORE DELETE ON events
BEGIN SELECT RAISE(ABORT, 'events are append-only'); END;
"#,
    r#"
ALTER TABLE asks ADD COLUMN asking_client TEXT;
ALTER TABLE asks ADD COLUMN asking_cwd TEXT;
"#,
    r#"
ALTER TABLE owners ADD COLUMN muted INTEGER NOT NULL DEFAULT 0;
"#,
    r#"
ALTER TABLE owners ADD COLUMN last_toast_ms INTEGER;
ALTER TABLE owners ADD COLUMN toast_head TEXT;
ALTER TABLE asks ADD COLUMN toast_notified INTEGER NOT NULL DEFAULT 0;
CREATE INDEX asks_toast_pending ON asks(status, toast_notified, deadline_ms);
UPDATE owners SET last_toast_ms = (SELECT MAX(t.created_ms) FROM toasts t JOIN asks a ON a.id = t.ask_id WHERE a.owner = owners.name);
UPDATE asks SET toast_notified = 1 WHERE id IN (SELECT ask_id FROM toasts);
"#,
    r#"
ALTER TABLE owners ADD COLUMN requested_owner TEXT;
CREATE TABLE owner_bindings (
    provisional TEXT PRIMARY KEY,
    owner TEXT NOT NULL REFERENCES owners(name)
) WITHOUT ROWID;
"#,
    r#"
ALTER TABLE deliveries ADD COLUMN confirmed INTEGER NOT NULL DEFAULT 0;
-- Older queue success records did not prove receipt. Preserve only explicit relay confirmations.
UPDATE deliveries SET confirmed = 1 WHERE ok = 1 AND (ask_id, target) IN (
    SELECT e.subject, json_extract(e.data, '$.to') FROM events e WHERE e.kind = 'delivery'
    AND json_extract(e.data, '$.by') = 'relay' AND json_extract(e.data, '$.ok') = 1
);
"#,
    r#"
CREATE TABLE board_profiles (
    handle TEXT PRIMARY KEY REFERENCES agents(handle) ON DELETE CASCADE,
    roles_json TEXT NOT NULL DEFAULT '[]',
    repo TEXT
);
CREATE TABLE board_limits (
    owner TEXT PRIMARY KEY,
    lease_seconds INTEGER NOT NULL DEFAULT 14400
);
CREATE TABLE board_requests (
    id TEXT PRIMARY KEY,
    owner TEXT NOT NULL,
    input_json TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'open',
    created_ms INTEGER NOT NULL,
    updated_ms INTEGER NOT NULL,
    claimant TEXT,
    fence INTEGER NOT NULL DEFAULT 0,
    lease_ms INTEGER,
    result TEXT
);
CREATE INDEX board_requests_status ON board_requests(status, updated_ms);
CREATE INDEX board_requests_owner ON board_requests(owner, status);
CREATE INDEX board_requests_claimant ON board_requests(claimant, status);
CREATE TABLE board_messages (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    owner TEXT NOT NULL,
    sender TEXT NOT NULL,
    input_json TEXT NOT NULL,
    created_ms INTEGER NOT NULL
);
CREATE INDEX board_messages_owner ON board_messages(owner);
"#,
    r#"
CREATE TABLE hold_scans(source TEXT PRIMARY KEY, result_json TEXT NOT NULL);
CREATE TABLE agent_holds(id TEXT PRIMARY KEY, owner TEXT NOT NULL, handle TEXT NOT NULL, repo TEXT NOT NULL, paths_json TEXT NOT NULL, note TEXT NOT NULL, expires_ms INTEGER NOT NULL);
CREATE INDEX agent_holds_repo ON agent_holds(repo,expires_ms);
"#,
    "ALTER TABLE asks ADD COLUMN asking_machine TEXT;",
    r#"
ALTER TABLE toasts ADD COLUMN purpose TEXT NOT NULL DEFAULT 'toast';
CREATE TABLE phone_outbox (
    ask_id TEXT PRIMARY KEY REFERENCES asks(id),
    toast_id TEXT NOT NULL REFERENCES toasts(toast_id),
    sent_ms INTEGER
);
CREATE TABLE phone_receipts (
    id TEXT PRIMARY KEY,
    ask_id TEXT NOT NULL REFERENCES asks(id),
    token TEXT NOT NULL,
    received_ms INTEGER NOT NULL,
    completed_ms INTEGER,
    outcome TEXT,
    acknowledged_ms INTEGER
);
"#,
    "ALTER TABLE owners ADD COLUMN muted_until_ms INTEGER; CREATE INDEX owners_mute_expiry ON owners(muted_until_ms) WHERE muted_until_ms IS NOT NULL;",
    "CREATE INDEX asks_history ON asks(closed_ms DESC,id DESC); CREATE INDEX board_request_history ON board_requests(updated_ms DESC,id DESC);",
    "ALTER TABLE phone_receipts ADD COLUMN toast_id TEXT; CREATE UNIQUE INDEX phone_receipt_notification ON phone_receipts(toast_id) WHERE toast_id IS NOT NULL;",
    // A Board reference belongs to the sender's computer, even after that registration is gone.
    "ALTER TABLE board_messages ADD COLUMN sender_machine TEXT;",
];

pub(crate) fn db_err(e: rusqlite::Error) -> String {
    format!("database error: {e}")
}

/// Open (creating if needed) the daemon's database file: parent folders, exclusive lock, WAL, migrations.
pub fn open_file(path: &Path) -> Result<Connection, String> {
    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
    {
        std::fs::create_dir_all(dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
    }
    let busy = |e: rusqlite::Error| {
        if matches!(
            e.sqlite_error_code(),
            Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)
        ) {
            format!(
                "{} is held by another process (another Agentbolt daemon?); only one daemon may open it",
                path.display()
            )
        } else {
            format!("can't open {}: {e}", path.display())
        }
    };
    let mut conn = Connection::open(path).map_err(busy)?;
    conn.busy_timeout(Duration::from_millis(250))
        .map_err(busy)?;
    // Exclusive mode must be set before the first WAL access, so SQLite keeps the WAL index in heap memory
    // and no other connection can share the file.
    let _: String = conn
        .query_row("PRAGMA locking_mode=EXCLUSIVE", [], |r| r.get(0))
        .map_err(busy)?;
    let mode: String = conn
        .query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))
        .map_err(busy)?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(format!("{} refused WAL mode (got {mode})", path.display()));
    }
    conn.execute_batch("PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;")
        .map_err(busy)?;
    // Take the exclusive lock now (it is held until the connection closes), so a second daemon fails at open.
    conn.execute_batch("BEGIN EXCLUSIVE; COMMIT;")
        .map_err(busy)?;
    migrate(&mut conn)?;
    Ok(conn)
}

/// An in-memory database with the same schema (tests).
pub fn open_memory() -> Result<Connection, String> {
    let mut conn = Connection::open_in_memory().map_err(db_err)?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")
        .map_err(db_err)?;
    migrate(&mut conn)?;
    Ok(conn)
}

fn migrate(conn: &mut Connection) -> Result<(), String> {
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(db_err)?;
    let known = MIGRATIONS.len() as i64;
    if version > known {
        return Err(format!(
            "the database is schema version {version}, newer than this Agentbolt understands ({known}); update Agentbolt"
        ));
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
        let tx = conn.transaction().map_err(db_err)?;
        tx.execute_batch(sql).map_err(db_err)?;
        tx.execute_batch(&format!("PRAGMA user_version = {}", i + 1))
            .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
    }
    Ok(())
}

// ---- time ----

pub fn to_ms(t: OffsetDateTime) -> i64 {
    (t.unix_timestamp_nanos() / 1_000_000) as i64
}

pub fn from_ms(ms: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp_nanos(ms as i128 * 1_000_000)
        .unwrap_or(OffsetDateTime::UNIX_EPOCH)
}

/// RFC 3339 in UTC, e.g. `2026-10-18T10:00:00Z`.
pub fn fmt_ms(ms: i64) -> String {
    from_ms(ms)
        .format(&Rfc3339)
        .unwrap_or_else(|_| ms.to_string())
}

// ---- enums as text ----

pub(crate) fn status_str(s: AskStatus) -> &'static str {
    match s {
        AskStatus::Open => "open",
        AskStatus::Digest => "digest",
        AskStatus::Answered => "answered",
        AskStatus::Defaulted => "defaulted",
        AskStatus::Cancelled => "cancelled",
    }
}

fn parse_status(s: &str) -> Option<AskStatus> {
    Some(match s {
        "open" => AskStatus::Open,
        "digest" => AskStatus::Digest,
        "answered" => AskStatus::Answered,
        "defaulted" => AskStatus::Defaulted,
        "cancelled" => AskStatus::Cancelled,
        _ => return None,
    })
}

pub(crate) fn via_str(v: AnswerVia) -> &'static str {
    match v {
        AnswerVia::Desk => "desk",
        AnswerVia::Toast => "toast",
        AnswerVia::Phone => "phone",
        AnswerVia::Defaulted => "defaulted",
        AnswerVia::NotNeeded => "not_needed",
    }
}

fn parse_via(s: &str) -> Option<AnswerVia> {
    Some(match s {
        "desk" => AnswerVia::Desk,
        "toast" => AnswerVia::Toast,
        "phone" => AnswerVia::Phone,
        "defaulted" => AnswerVia::Defaulted,
        "not_needed" => AnswerVia::NotNeeded,
        _ => return None,
    })
}

fn bad(idx: usize, what: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(idx, Type::Text, format!("unexpected {what}").into())
}

// ---- asks ----

/// An ask as stored, with the columns the rules need.
#[derive(Debug, Clone)]
pub(crate) struct AskRow {
    pub id: String,
    pub owner: String,
    pub muted: bool,
    pub asking_session: Option<String>,
    pub asking_client: Option<String>,
    pub asking_cwd: Option<String>,
    pub asking_machine: Option<String>,
    pub input: AskInput,
    pub status: AskStatus,
    pub created_ms: i64,
    pub shown_ms: Option<i64>,
    pub deadline_ms: i64,
}

const ASK_COLS: &str = "id, owner, asking_session, input_json, status, created_ms, shown_ms, deadline_ms, asking_client, asking_cwd, COALESCE((SELECT muted FROM owners WHERE name = asks.owner), 0) AS muted, asking_machine";

fn map_ask(r: &Row<'_>) -> rusqlite::Result<AskRow> {
    let input_json: String = r.get(3)?;
    let input: AskInput = serde_json::from_str(&input_json)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(3, Type::Text, Box::new(e)))?;
    let status: String = r.get(4)?;
    Ok(AskRow {
        id: r.get(0)?,
        owner: r.get(1)?,
        muted: r.get(10)?,
        asking_session: r.get(2)?,
        asking_client: r.get(8)?,
        asking_cwd: r.get(9)?,
        asking_machine: r.get(11)?,
        input,
        status: parse_status(&status).ok_or_else(|| bad(4, "ask status"))?,
        created_ms: r.get(5)?,
        shown_ms: r.get(6)?,
        deadline_ms: r.get(7)?,
    })
}

/// Asks matching `clause` (a WHERE body, optionally followed by ORDER BY / LIMIT; it may refer to `asks`).
pub(crate) fn ask_rows<P: Params>(
    conn: &Connection,
    clause: &str,
    params: P,
) -> rusqlite::Result<Vec<AskRow>> {
    let mut stmt = conn.prepare(&format!("SELECT {ASK_COLS} FROM asks WHERE {clause}"))?;
    let rows = stmt.query_map(params, map_ask)?;
    rows.collect()
}

pub(crate) fn ask_row(conn: &Connection, id: &str) -> rusqlite::Result<Option<AskRow>> {
    conn.query_row(
        &format!("SELECT {ASK_COLS} FROM asks WHERE id = ?1"),
        [id],
        map_ask,
    )
    .optional()
}

pub(crate) struct NewAsk<'a> {
    pub id: &'a str,
    pub owner: &'a str,
    pub asking_session: Option<&'a str>,
    pub asking_client: Option<&'a str>,
    pub asking_cwd: Option<&'a str>,
    pub asking_machine: Option<&'a str>,
    pub input: &'a AskInput,
    pub p0_skip: bool,
    pub deadline_ms: i64,
    pub status: AskStatus,
    pub created_ms: i64,
    pub inbox_ms: Option<i64>,
}

pub(crate) fn insert_ask(conn: &Connection, a: &NewAsk<'_>) -> rusqlite::Result<()> {
    let input_json = serde_json::to_string(a.input)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    let priority = rules::priority_str(a.input.priority);
    conn.execute(
        "INSERT INTO asks (id, owner, asking_session, input_json, priority, p0_skip, reversible, deadline_ms, status,
                           created_ms, inbox_ms, asking_client, asking_cwd, asking_machine)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            a.id,
            a.owner,
            a.asking_session,
            input_json,
            priority,
            a.p0_skip,
            a.input.reversible,
            a.deadline_ms,
            status_str(a.status),
            a.created_ms,
            a.inbox_ms,
            a.asking_client,
            a.asking_cwd,
            a.asking_machine
        ],
    )?;
    Ok(())
}

pub(crate) fn answer_for(conn: &Connection, ask_id: &str) -> rusqlite::Result<Option<Answer>> {
    let row = conn
        .query_row(
            "SELECT via, choice, values_json, note, answered_ms FROM answers WHERE ask_id = ?1",
            [ask_id],
            |r| {
                let via: String = r.get(0)?;
                let values: Option<String> = r.get(2)?;
                let values = match values {
                    Some(v) => {
                        Some(serde_json::from_str::<serde_json::Value>(&v).map_err(|e| {
                            rusqlite::Error::FromSqlConversionFailure(2, Type::Text, Box::new(e))
                        })?)
                    }
                    None => None,
                };
                Ok((
                    parse_via(&via).ok_or_else(|| bad(0, "answer via"))?,
                    r.get::<_, Option<String>>(1)?,
                    values,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, i64>(4)?,
                ))
            },
        )
        .optional()?;
    let Some((via, choice, values, note, answered_ms)) = row else {
        return Ok(None);
    };
    let history = delivery_page(conn, ask_id, None)?;
    Ok(Some(Answer {
        ask_id: ask_id.to_string(),
        via,
        choice,
        values,
        note,
        answered_at: fmt_ms(answered_ms),
        deliveries: history.deliveries,
        delivery_total: history.total,
        delivery_next: history.next,
        more_deliveries: history.more,
    }))
}

#[cfg(test)]
pub(crate) fn deliveries_for(conn: &Connection, ask_id: &str) -> rusqlite::Result<Vec<Delivery>> {
    Ok(delivery_page(conn, ask_id, None)?.deliveries)
}

pub(crate) fn delivery_page(
    conn: &Connection,
    ask_id: &str,
    before: Option<u64>,
) -> rusqlite::Result<crate::protocol::DeliveryPage> {
    let before = before.map(i64::try_from).transpose().map_err(|_| {
        rusqlite::Error::InvalidParameterName("delivery cursor is too large".into())
    })?;
    let total = count(
        conn,
        "SELECT COUNT(*) FROM deliveries WHERE ask_id=?1",
        [ask_id],
    )?;
    let mut stmt = conn.prepare("SELECT id,target,at_ms,ok,detail,confirmed FROM deliveries WHERE ask_id=?1 AND (?2 IS NULL OR id<?2) ORDER BY id DESC LIMIT 51")?;
    let mut rows = stmt
        .query_map(params![ask_id, before], |r| {
            Ok((
                r.get::<_, i64>(0)? as u64,
                Delivery {
                    to: r.get(1)?,
                    at: fmt_ms(r.get(2)?),
                    ok: r.get(3)?,
                    detail: r.get(4)?,
                    confirmed: r.get(5)?,
                },
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let more = rows.len() > 50;
    rows.truncate(50);
    let next = rows.last().map(|r| r.0);
    // Keep chronological order within a page, including the legacy short-history ordering.
    rows.reverse();
    Ok(crate::protocol::DeliveryPage {
        deliveries: rows.into_iter().map(|r| r.1).collect(),
        next,
        more,
        total,
    })
}

pub(crate) fn insert_answer(
    conn: &Connection,
    a: &Answer,
    answered_ms: i64,
) -> rusqlite::Result<()> {
    let values = a.values.as_ref().map(|v| v.to_string());
    conn.execute(
        "INSERT INTO answers (ask_id, via, choice, values_json, note, answered_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![a.ask_id, via_str(a.via), a.choice, values, a.note, answered_ms],
    )?;
    Ok(())
}

pub(crate) fn insert_delivery(
    conn: &Connection,
    ask_id: &str,
    target: &str,
    at_ms: i64,
    ok: bool,
    confirmed: bool,
    detail: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO deliveries (ask_id, target, at_ms, ok, confirmed, detail) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![ask_id, target, at_ms, ok, confirmed, detail],
    )?;
    Ok(())
}

pub(crate) fn count<P: Params>(conn: &Connection, sql: &str, params: P) -> rusqlite::Result<u32> {
    let n: i64 = conn.query_row(sql, params, |r| r.get(0))?;
    Ok(n.max(0) as u32)
}

// ---- owners ----

fn map_owner(r: &Row<'_>) -> rusqlite::Result<Owner> {
    Ok(Owner {
        name: r.get(0)?,
        binding: r.get(1)?,
        codex_interactive: r.get(2)?,
        relay: r.get(3)?,
        muted: r.get(4)?,
        not_needed_count: r.get(5)?,
        requested_owner: r.get(6)?,
        muted_until: r.get::<_, Option<i64>>(7)?.map(fmt_ms),
    })
}

pub(crate) fn owner(conn: &Connection, name: &str) -> rusqlite::Result<Option<Owner>> {
    conn.query_row(
        "SELECT name, binding, codex_interactive, relay, muted,
         (SELECT COUNT(*) FROM answers a JOIN asks k ON k.id = a.ask_id WHERE k.owner = owners.name AND a.via = 'not_needed'), requested_owner, muted_until_ms
         FROM owners WHERE name = ?1",
        [name],
        map_owner,
    )
    .optional()
}

/// Owners Artem has to keep seeing: bound by config, approved, or given a flag on the Desk.
const KEPT_OWNER: &str =
    "(binding <> 'pending' OR codex_interactive = 1 OR relay = 1 OR muted = 1)";

/// An owner's latest activity: created, an agent seen, or an ask made.
const OWNER_ACTIVITY: &str = "MAX(o.created_ms,
         COALESCE((SELECT MAX(a.last_seen_ms) FROM agents a WHERE a.owner = o.name), 0),
         COALESCE((SELECT MAX(k.created_ms) FROM asks k WHERE k.owner = o.name), 0))";

/// The Desk's owners list: every kept owner by name, then the `recent` most recently active provisional ones,
/// newest first. Also returns how many provisional owners were left out.
pub(crate) fn owners_for_desk(
    conn: &Connection,
    recent: u32,
) -> rusqlite::Result<(Vec<Owner>, u32)> {
    let mut kept = conn.prepare(&format!(
        "SELECT name, binding, codex_interactive, relay, muted,
         (SELECT COUNT(*) FROM answers a JOIN asks k ON k.id = a.ask_id WHERE k.owner = owners.name AND a.via = 'not_needed'), requested_owner, muted_until_ms
         FROM owners WHERE {KEPT_OWNER} ORDER BY name"
    ))?;
    let mut owners: Vec<Owner> = kept
        .query_map([], map_owner)?
        .collect::<rusqlite::Result<_>>()?;
    let mut provisional = conn.prepare(&format!(
        "SELECT o.name, o.binding, o.codex_interactive, o.relay, o.muted,
         (SELECT COUNT(*) FROM answers a JOIN asks k ON k.id = a.ask_id WHERE k.owner = o.name AND a.via = 'not_needed'), o.requested_owner, o.muted_until_ms
         FROM owners o WHERE NOT {KEPT_OWNER}
         ORDER BY {OWNER_ACTIVITY} DESC, o.name LIMIT ?1"
    ))?;
    let shown: Vec<Owner> = provisional
        .query_map([recent], map_owner)?
        .collect::<rusqlite::Result<_>>()?;
    let all = count(
        conn,
        &format!("SELECT COUNT(*) FROM owners WHERE NOT {KEPT_OWNER}"),
        [],
    )?;
    let more = all.saturating_sub(shown.len() as u32);
    owners.extend(shown);
    Ok((owners, more))
}

/// Delete provisional owners with no asks at all and no activity since `before_ms`. Returns how many went.
pub(crate) fn prune_provisional_owners(
    conn: &Connection,
    before_ms: i64,
) -> rusqlite::Result<usize> {
    conn.execute(
        &format!(
            "DELETE FROM owners WHERE NOT {KEPT_OWNER} AND created_ms < ?1
               AND NOT EXISTS (SELECT 1 FROM asks WHERE asks.owner = owners.name)
               AND NOT EXISTS (SELECT 1 FROM board_requests WHERE board_requests.owner = owners.name)
               AND NOT EXISTS (SELECT 1 FROM board_messages WHERE board_messages.owner = owners.name)
               AND NOT EXISTS (SELECT 1 FROM agents WHERE agents.owner = owners.name AND agents.last_seen_ms >= ?1)"
        ),
        [before_ms],
    )
}

pub(crate) fn upsert_owner(conn: &Connection, o: &Owner, now_ms: i64) -> rusqlite::Result<()> {
    let until = o
        .muted_until
        .as_deref()
        .map(|s| OffsetDateTime::parse(s, &Rfc3339).map(to_ms))
        .transpose()
        .map_err(|_| rusqlite::Error::InvalidParameterName("muted_until".into()))?;
    conn.execute(
        "INSERT INTO owners (name, binding, codex_interactive, relay, created_ms, muted, muted_until_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(name) DO UPDATE SET binding = excluded.binding, codex_interactive = excluded.codex_interactive,
                                         relay = excluded.relay, muted = excluded.muted, muted_until_ms=excluded.muted_until_ms",
        params![o.name, o.binding, o.codex_interactive, o.relay, now_ms, o.muted,until],
    )?;
    Ok(())
}

pub(crate) fn approved_owner(
    conn: &Connection,
    provisional: &str,
) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT owner FROM owner_bindings WHERE provisional = ?1",
        [provisional],
        |r| r.get(0),
    )
    .optional()
}

// ---- agents ----

const AGENT_SELECT: &str =
    "SELECT a.handle, a.owner, COALESCE(o.binding, 'pending'), a.client, a.session_ref, a.cwd,
                                   a.machine, a.doing, a.registered_ms, a.last_seen_ms, COALESCE(o.muted, 0), o.requested_owner
                            FROM agents a LEFT JOIN owners o ON o.name = a.owner";

fn map_agent(r: &Row<'_>) -> rusqlite::Result<Agent> {
    let binding: String = r.get(2)?;
    Ok(Agent {
        handle: r.get(0)?,
        owner: r.get(1)?,
        provisional: binding == "pending",
        muted: r.get(10)?,
        requested_owner: r.get(11)?,
        client: r.get(3)?,
        session_ref: r.get(4)?,
        cwd: r.get(5)?,
        machine: r.get(6)?,
        doing: r.get(7)?,
        registered_at: fmt_ms(r.get(8)?),
        last_seen: fmt_ms(r.get(9)?),
    })
}

pub(crate) fn agent(conn: &Connection, handle: &str) -> rusqlite::Result<Option<Agent>> {
    conn.query_row(
        &format!("{AGENT_SELECT} WHERE a.handle = ?1"),
        [handle],
        map_agent,
    )
    .optional()
}

pub(crate) fn agents(conn: &Connection) -> rusqlite::Result<Vec<Agent>> {
    let mut stmt = conn.prepare(&format!(
        "{AGENT_SELECT} ORDER BY a.last_seen_ms DESC, a.handle"
    ))?;
    let rows = stmt.query_map([], map_agent)?;
    rows.collect()
}

fn directory_search(query: &crate::protocol::DirectoryQuery) -> Result<&str, String> {
    let search = query.search.trim();
    if search.len() > 800 || search.chars().any(char::is_control) {
        return Err("Search needs at most 800 bytes without control characters".into());
    }
    Ok(search)
}

pub(crate) fn agent_directory(
    conn: &Connection,
    query: crate::protocol::DirectoryQuery,
) -> Result<crate::protocol::DirectoryPage<Agent>, String> {
    let search = directory_search(&query)?;
    let before = query
        .after
        .as_deref()
        .map(str::parse::<i64>)
        .transpose()
        .map_err(|_| "Invalid agent page cursor")?;
    let filter = "(?1='' OR instr(lower(owner||' '||client||' '||machine||' '||COALESCE(session_ref,'')||' '||COALESCE(doing,'')),lower(?1))>0)";
    let total = count(
        conn,
        &format!("SELECT COUNT(*) FROM agents WHERE {filter}"),
        [search],
    )
    .map_err(db_err)?;
    let mut stmt = conn.prepare(&format!("SELECT rowid,handle FROM agents WHERE {filter} AND (?2 IS NULL OR rowid<?2) ORDER BY rowid DESC LIMIT 51")).map_err(db_err)?;
    let mut rows = stmt
        .query_map(params![search, before], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(db_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_err)?;
    let more = rows.len() > 50;
    rows.truncate(50);
    let next = rows.last().map(|r| r.0.to_string());
    let items = rows
        .into_iter()
        .map(|(_, handle)| {
            agent(conn, &handle)
                .map_err(db_err)?
                .ok_or_else(|| "Agent disappeared".into())
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(crate::protocol::DirectoryPage {
        items,
        next,
        more,
        total,
    })
}

pub(crate) fn owner_directory(
    conn: &Connection,
    query: crate::protocol::DirectoryQuery,
) -> Result<crate::protocol::DirectoryPage<Owner>, String> {
    let search = directory_search(&query)?;
    let filter = "(?1='' OR instr(lower(name||' '||binding||' '||COALESCE(requested_owner,'')),lower(?1))>0)";
    let total = count(
        conn,
        &format!("SELECT COUNT(*) FROM owners WHERE {filter}"),
        [search],
    )
    .map_err(db_err)?;
    let mut stmt = conn.prepare(&format!("SELECT name FROM owners WHERE {filter} AND (?2 IS NULL OR name>?2) ORDER BY name LIMIT 51")).map_err(db_err)?;
    let mut rows = stmt
        .query_map(params![search, query.after], |r| r.get::<_, String>(0))
        .map_err(db_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_err)?;
    let more = rows.len() > 50;
    rows.truncate(50);
    let next = rows.last().cloned();
    let items = rows
        .into_iter()
        .map(|name| {
            owner(conn, &name)
                .map_err(db_err)?
                .ok_or_else(|| "Owner disappeared".into())
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(crate::protocol::DirectoryPage {
        items,
        next,
        more,
        total,
    })
}

pub(crate) struct NewAgent<'a> {
    pub handle: &'a str,
    pub owner: &'a str,
    pub client: &'a str,
    pub session_ref: Option<&'a str>,
    pub cwd: Option<&'a str>,
    pub machine: &'a str,
    pub doing: Option<&'a str>,
    pub now_ms: i64,
    pub connection: i64,
}

pub(crate) fn insert_agent(conn: &Connection, a: &NewAgent<'_>) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO agents (handle, owner, client, session_ref, cwd, machine, doing, registered_ms, last_seen_ms,
                             connection)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, ?9)",
        params![a.handle, a.owner, a.client, a.session_ref, a.cwd, a.machine, a.doing, a.now_ms, a.connection],
    )?;
    Ok(())
}

// ---- meta and settings ----

pub(crate) fn meta(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
        .optional()
}

pub(crate) fn set_meta(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub(crate) fn settings(conn: &Connection) -> rusqlite::Result<Settings> {
    let stored = meta(conn, "settings")?.and_then(|s| serde_json::from_str::<Settings>(&s).ok());
    Ok(stored.unwrap_or_default())
}

pub(crate) fn set_settings(conn: &Connection, s: &Settings) -> rusqlite::Result<()> {
    let json = serde_json::to_string(s)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    set_meta(conn, "settings", &json)
}

#[cfg(test)]
mod migration_tests {
    use super::*;

    #[test]
    fn board_message_origin_upgrade_preserves_legacy_content_without_guessing_a_machine() {
        let mut conn = Connection::open_in_memory().unwrap();
        for migration in &MIGRATIONS[..13] {
            conn.execute_batch(migration).unwrap();
        }
        conn.execute_batch("PRAGMA user_version=13;").unwrap();
        let input = serde_json::json!({"title":"Legacy reference","body":"Keep this message","references":["C:/DEV/evidence.md"]}).to_string();
        conn.execute("INSERT INTO board_messages(id,owner,sender,input_json,created_ms) VALUES ('msg_old','legacy','agent_old',?1,900)", [&input]).unwrap();
        migrate(&mut conn).unwrap();
        migrate(&mut conn).unwrap();
        let snapshot =
            crate::board::snapshot_page(&conn, 1000, crate::board::DeskQuery::default()).unwrap();
        assert_eq!(snapshot.messages.len(), 1);
        let message = &snapshot.messages[0];
        assert_eq!(message.id, "msg_old");
        assert_eq!(message.input.body, "Keep this message");
        assert_eq!(message.input.references, ["C:/DEV/evidence.md"]);
        assert_eq!(message.sender_machine, None);
        let unchanged: String = conn
            .query_row(
                "SELECT input_json FROM board_messages WHERE id='msg_old'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(unchanged, input);
    }

    #[test]
    fn phone_receipt_upgrade_preserves_pending_and_completed_legacy_receipts() {
        let mut conn = Connection::open_in_memory().unwrap();
        for migration in &MIGRATIONS[..12] {
            conn.execute_batch(migration).unwrap();
        }
        conn.execute_batch("PRAGMA user_version=12;").unwrap();
        let input = serde_json::json!({"kind":"choose","title":"Legacy phone question","options":["Yes","No"],"default":"No","deadline":"2026-10-20T00:00:00Z","reversible":true,"priority":"P1"});
        conn.execute("INSERT INTO asks(id,owner,input_json,priority,reversible,deadline_ms,status,created_ms) VALUES ('ask_old','legacy',?1,'P1',1,3000,'open',900),('ask_closed','legacy',?1,'P1',1,3000,'answered',900)",[input.to_string()]).unwrap();
        conn.execute_batch("INSERT INTO answers(ask_id,via,choice,answered_ms) VALUES ('ask_closed','phone','Yes',1100);").unwrap();
        conn.execute_batch("INSERT INTO phone_receipts(id,ask_id,token,received_ms,completed_ms,outcome) VALUES ('pending','ask_old','opaque-unconsumed-token',1000,NULL,NULL),('completed','ask_closed','',1000,1100,'Answered');").unwrap();
        migrate(&mut conn).unwrap();
        migrate(&mut conn).unwrap();
        assert_eq!(
            crate::phone::receipt(&conn, "pending").unwrap(),
            Some(("ask_old".into(), "opaque-unconsumed-token".into(), None))
        );
        assert_eq!(
            crate::phone::receipt(&conn, "completed").unwrap(),
            Some(("ask_closed".into(), String::new(), Some(1100)))
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM phone_receipts WHERE toast_id IS NULL",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            2
        );
        conn.execute(
            "UPDATE phone_receipts SET toast_id='notification' WHERE id='pending'",
            [],
        )
        .unwrap();
        assert!(conn.execute("INSERT INTO phone_receipts(id,ask_id,token,received_ms,toast_id) VALUES ('duplicate','ask_old','other-option',2000,'notification')",[]).is_err());
        assert_eq!(
            crate::phone::receipt(&conn, "pending").unwrap().unwrap().1,
            "opaque-unconsumed-token"
        );
    }

    #[test]
    fn released_database_upgrade_preserves_answers_and_never_guesses_a_chat() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        conn.execute_batch("PRAGMA user_version = 1;").unwrap();
        let input = serde_json::json!({
            "kind":"choose", "title":"Existing decision", "options":["Yes","No"],
            "default":"No", "deadline":"2026-10-20T00:00:00Z", "reversible":true,
            "priority":"P1"
        });
        conn.execute(
            "INSERT INTO asks (id, owner, asking_session, input_json, priority, reversible,
             deadline_ms, status, created_ms, inbox_ms, shown_ms, closed_ms)
             VALUES ('old', 'owner', 'original-host-session', ?1, 'P1', 1, 2000,
                     'answered', 1000, 1000, 1100, 1200)",
            [input.to_string()],
        )
        .unwrap();
        conn.execute_batch(
            "INSERT INTO answers (ask_id, via, choice, answered_ms)
                            VALUES ('old', 'desk', 'No', 1200);
             INSERT INTO deliveries (ask_id, target, at_ms, ok, detail) VALUES
                 ('old', 'original-host-session', 1300, 1, 'codex queue exited 0'),
                 ('old', 'notes', 1400, 1, 'written to notes'),
                 ('old', 'failed-target', 1500, 0, 'failed'),
                 ('old', 'unknown-target', 1600, 1, 'legacy record without provenance');",
        )
        .unwrap();
        for (at, target, ok, by) in [
            (1300, "original-host-session", true, "daemon"),
            (1400, "notes", true, "relay"),
            (1500, "failed-target", false, "relay"),
        ] {
            crate::events::append(
                &conn,
                at,
                "delivery",
                "old",
                &serde_json::json!({"to": target, "ok": ok, "by": by}),
            )
            .unwrap();
        }
        migrate(&mut conn).unwrap();
        migrate(&mut conn).unwrap(); // A second launch must be harmless.
        let ask = ask_row(&conn, "old").unwrap().unwrap();
        assert_eq!(ask.status, AskStatus::Answered);
        assert_eq!(ask.shown_ms, Some(1100));
        assert_eq!(ask.asking_session.as_deref(), Some("original-host-session"));
        assert!(ask.asking_client.is_none() && ask.asking_cwd.is_none());
        let choice: String = conn
            .query_row("SELECT choice FROM answers WHERE ask_id = 'old'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(choice, "No");
        let deliveries = deliveries_for(&conn, "old").unwrap();
        assert_eq!(
            deliveries
                .iter()
                .map(|d| (d.ok, d.confirmed))
                .collect::<Vec<_>>(),
            [(true, false), (true, true), (false, false), (true, false)]
        );
        assert!(
            crate::events::verify_chain(&conn).is_ok(),
            "migration leaves the audit chain intact"
        );
        assert_eq!(
            conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            MIGRATIONS.len() as i64
        );
    }
}
