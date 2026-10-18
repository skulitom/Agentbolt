//! Opt-in phone transport state. No worker operation can write a human answer: a received token is
//! queued durably and only `DeskOp::PhoneReply` can consume it. Credentials never enter audit events.
use crate::{
    events,
    model::AskInput,
    rules,
    store::{self, db_err, fmt_ms},
    tokens,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const PROTECTION_TTL_MS: i64 = 120_000;
const BATCH_MS: i64 = 600_000;

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Secret(pub String);
impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[redacted]")
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub enabled: bool,
    pub server: String,
    pub questions: String,
    pub replies: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Update {
    pub settings: Settings,
    /// None preserves the saved credential; it is never returned to the Desk.
    pub daemon_token: Option<Secret>,
    pub reply_token: Option<Secret>,
    #[serde(default)]
    pub forget_credentials: bool,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    pub settings: Settings,
    pub daemon_token: Secret,
    pub reply_token: Secret,
    pub generation: String,
    pub since: i64,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Health {
    pub protected_at: Option<i64>,
    pub polled_at: Option<i64>,
    pub published_at: Option<i64>,
    pub error: Option<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Status {
    pub settings: Settings,
    pub has_credentials: bool,
    pub health: Health,
    pub pending_replies: u32,
}
#[derive(Debug, Clone)]
pub struct State {
    pub config: Config,
    pub health: Health,
    pub cursor: Option<String>,
}
pub struct Notification {
    pub ask_id: String,
    pub toast_id: String,
    pub title: String,
    pub body: String,
    pub buttons: Vec<(String, Secret)>,
}
pub struct Incoming {
    pub id: String,
    pub token: Secret,
}
#[derive(Debug, Clone)]
pub struct Acknowledgement {
    pub id: String,
    pub ask_id: String,
    pub outcome: String,
}

/// Private, in-process daemon API. Deliberately absent from both pipe protocols.
pub enum Work {
    State,
    Verified,
    Fault(String),
    Prepare,
    Published(String),
    Receive {
        messages: Vec<Incoming>,
        cursor: Option<String>,
    },
    Pending,
    Acknowledged(String),
}
pub enum ResultValue {
    State(State),
    Notification(Option<Notification>),
    Pending {
        ids: Vec<String>,
        acknowledgements: Vec<Acknowledgement>,
    },
    Done,
}

fn read<T: serde::de::DeserializeOwned + Default>(db: &Connection, key: &str) -> Result<T, String> {
    store::meta(db, key)
        .map_err(db_err)?
        .map(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
        .unwrap_or_else(|| Ok(T::default()))
}
pub fn config(db: &Connection) -> Result<Config, String> {
    read(db, "phone_config")
}
pub fn status(db: &Connection) -> Result<Status, String> {
    let cfg = config(db)?;
    Ok(Status {
        has_credentials: !cfg.daemon_token.0.is_empty() && !cfg.reply_token.0.is_empty(),
        settings: cfg.settings,
        health: read(db, "phone_health")?,
        pending_replies: db
            .query_row(
                "SELECT COUNT(*) FROM phone_receipts WHERE completed_ms IS NULL",
                [],
                |r| r.get(0),
            )
            .map_err(db_err)?,
    })
}

pub fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
/// An HTTPS origin only. Keeping paths, credentials, queries, escapes and fragments out also makes
/// the daemon's fixed route construction unambiguous. IPv6 literals are accepted in brackets.
pub fn server(value: &str) -> Result<String, String> {
    let value = value.trim().trim_end_matches('/');
    let origin = rules::https_lowercase_scheme(value).ok_or("use an https:// ntfy server")?;
    let authority = &origin[8..];
    if authority.is_empty()
        || authority.len() > 255
        || !authority
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-:[]".contains(&b))
    {
        return Err(
            "use an HTTPS server name and optional port, without a path or credentials".into(),
        );
    }
    Ok(origin)
}
pub fn set_config(db: &Connection, now: i64, update: Update) -> Result<Status, String> {
    let old = config(db)?;
    let new_credentials = update.daemon_token.is_some() && update.reply_token.is_some();
    let mut cfg = Config {
        settings: update.settings,
        daemon_token: old.daemon_token,
        reply_token: old.reply_token,
        generation: tokens::random_hex(16),
        since: now / 1000 - 5,
    };
    if update.forget_credentials {
        cfg.settings.enabled = false;
        cfg.daemon_token = Secret::default();
        cfg.reply_token = Secret::default();
    } else {
        if let Some(token) = update.daemon_token {
            cfg.daemon_token = token;
        }
        if let Some(token) = update.reply_token {
            cfg.reply_token = token;
        }
    }
    if cfg.settings.server.trim_end_matches('/') != old.settings.server.trim_end_matches('/')
        && !new_credentials
    {
        // Even a disabled draft must not move saved credentials to a different server on a later enable.
        cfg.daemon_token = Secret::default();
        cfg.reply_token = Secret::default();
    }
    if cfg.settings.enabled {
        cfg.settings.server = server(&cfg.settings.server)?;
        if !identifier(&cfg.settings.questions)
            || !identifier(&cfg.settings.replies)
            || cfg.settings.questions == cfg.settings.replies
        {
            return Err(
                "choose two different topic names (1–64 letters, digits, '-' or '_')".into(),
            );
        }
        if cfg.settings.server != old.settings.server && !new_credentials {
            return Err("enter new credentials when changing the server".into());
        }
        for token in [&cfg.daemon_token, &cfg.reply_token] {
            if token.0.is_empty()
                || token.0.len() > 4096
                || !token.0.bytes().all(|b| b.is_ascii_graphic())
            {
                return Err("enter both ntfy access tokens; tokens cannot contain spaces or control characters".into());
            }
        }
    }
    db.execute("UPDATE toasts SET revoked_ms=?1 WHERE purpose='phone' AND used_ms IS NULL AND revoked_ms IS NULL", [now]).map_err(db_err)?;
    db.execute("DELETE FROM phone_outbox", []).map_err(db_err)?;
    db.execute("UPDATE phone_receipts SET completed_ms=?1, outcome='Phone settings changed; answer in the Desk', acknowledged_ms=?1, token='' WHERE completed_ms IS NULL", [now]).map_err(db_err)?;
    db.execute(
        "UPDATE phone_receipts SET acknowledged_ms=?1 WHERE acknowledged_ms IS NULL",
        [now],
    )
    .map_err(db_err)?;
    store::set_meta(db, "phone_config", &json!(cfg).to_string()).map_err(db_err)?;
    store::set_meta(db, "phone_health", &json!(Health::default()).to_string()).map_err(db_err)?;
    db.execute("DELETE FROM meta WHERE key='phone_cursor'", [])
        .map_err(db_err)?;
    events::append(
        db,
        now,
        "phone.configured",
        "phone",
        &json!({"enabled":cfg.settings.enabled}),
    )
    .map_err(db_err)?;
    status(db)
}

/// Phone buttons inherit the same-user notification-token boundary. Full, short reversible choices
/// can be answered; irreversible, long, form and text questions remain in the Desk.
pub fn buttons_allowed(input: &AskInput) -> bool {
    rules::toast_buttons_allowed(input)
        && input.options.len() <= 3
        && input.options.iter().all(|o| o.chars().count() <= 64)
}
fn question_text(row: &store::AskRow) -> String {
    let mut body = format!(
        "From {}\n{}\n\n{}\n\nDue: {}",
        row.owner, row.input.title, row.input.body, row.input.deadline
    );
    body.push_str(&format!(
        "\nSuggested answer (not approval): {}",
        row.input.default
    ));
    if let Some(reason) = &row.input.p0_reason {
        body.push_str(&format!("\nUrgency: {reason}"));
    }
    for (i, option) in row.input.options.iter().enumerate() {
        body.push_str(&format!("\n{}. {option}", i + 1));
    }
    for evidence in &row.input.evidence {
        body.push_str(&format!("\n{}: {}", evidence.label, evidence.url));
    }
    body
}
fn can_offer(row: &store::AskRow) -> bool {
    buttons_allowed(&row.input) && question_text(row).len() <= 3500
}

fn prepare(db: &Connection, now: i64, secret: &[u8]) -> Result<Option<Notification>, String> {
    if store::settings(db).map_err(db_err)?.toasts_off {
        return Ok(None);
    }
    let recent: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM phone_outbox WHERE sent_ms>?1)",
            [now - 60_000],
            |r| r.get(0),
        )
        .map_err(db_err)?;
    if recent {
        return Ok(None);
    }
    let rows = store::ask_rows(db, "status='open' AND owner IN (SELECT name FROM owners WHERE muted=0) AND id NOT IN (SELECT ask_id FROM phone_outbox WHERE sent_ms IS NOT NULL) AND owner NOT IN (SELECT a.owner FROM phone_outbox p JOIN asks a ON a.id=p.ask_id WHERE p.sent_ms>?1) ORDER BY priority,deadline_ms,created_ms LIMIT 100", [now-BATCH_MS]).map_err(db_err)?;
    let Some(row) = rows
        .into_iter()
        .find(|r| rules::toast_worthy(&r.input, r.deadline_ms, now))
    else {
        return Ok(None);
    };
    let saved: Option<String> = db.query_row("SELECT p.toast_id FROM phone_outbox p JOIN toasts t ON t.toast_id=p.toast_id WHERE p.ask_id=?1 AND t.expires_ms>?2 AND t.revoked_ms IS NULL", params![row.id,now], |r| r.get(0)).optional().map_err(db_err)?;
    let toast_id = if let Some(id) = saved {
        id
    } else {
        let id = tokens::new_toast_id();
        db.execute("INSERT INTO toasts (toast_id,ask_id,created_ms,expires_ms,purpose) VALUES (?1,?2,?3,?4,'phone')", params![id,row.id,now,now+tokens::TOKEN_TTL_MS]).map_err(db_err)?;
        db.execute("INSERT INTO phone_outbox (ask_id,toast_id) VALUES (?1,?2) ON CONFLICT(ask_id) DO UPDATE SET toast_id=excluded.toast_id,sent_ms=NULL", params![row.id,id]).map_err(db_err)?;
        events::append(db, now, "phone.prepared", &row.id, &json!({})).map_err(db_err)?;
        id
    };
    let mut body = question_text(&row);
    let buttons = if can_offer(&row) {
        row.input
            .options
            .iter()
            .enumerate()
            .map(|(i, o)| {
                (
                    o.clone(),
                    Secret(tokens::mint(secret, &row.id, i, &toast_id)),
                )
            })
            .collect()
    } else {
        body = format!(
            "From {}\n{}\n\nThis question needs the Desk. Open Agentbolt on your computer to read the full question and answer.\nDue: {}",
            row.owner, row.input.title, row.input.deadline
        );
        Vec::new()
    };
    let count: u32 = db
        .query_row(
            "SELECT COUNT(*) FROM asks WHERE owner=?1 AND status='open' AND id<>?2",
            params![row.owner, row.id],
            |r| r.get(0),
        )
        .map_err(db_err)?;
    if count > 0 {
        body.push_str(&format!("\n\n{count} more waiting from this owner."));
    }
    body.push_str(&format!("\nQuestion: {}", row.id));
    Ok(Some(Notification {
        ask_id: row.id,
        toast_id,
        title: rules::clean_line(&row.input.title, 120),
        body,
        buttons,
    }))
}

/// Accept only a valid phone token issued by this database; arbitrary topic messages are never
/// executable instructions. The cursor and accepted receipts commit together.
fn receive(
    db: &Connection,
    now: i64,
    secret: &[u8],
    messages: Vec<Incoming>,
    cursor: Option<String>,
) -> Result<(), String> {
    if messages.len() > 500 {
        return Err("phone reply page exceeds 500 messages; no cursor advanced".into());
    }
    for message in messages {
        if !identifier(&message.id) {
            continue;
        }
        let Ok(parsed) = tokens::parse(&message.token.0) else {
            continue;
        };
        let ask: Option<String> = db
            .query_row(
                "SELECT ask_id FROM toasts WHERE toast_id=?1 AND purpose='phone' AND expires_ms>?2",
                params![parsed.toast_id, now],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_err)?;
        let Some(ask_id) = ask else {
            continue;
        };
        if !tokens::verify(secret, &ask_id, &parsed) {
            continue;
        }
        let row = store::ask_row(db, &ask_id)
            .map_err(db_err)?
            .ok_or("phone ask unavailable")?;
        if !can_offer(&row) || parsed.option_index >= row.input.options.len() {
            continue;
        }
        // One response per issued notification, including two quick taps on different options.
        let seen: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM phone_receipts WHERE ask_id=?1)",
                [&ask_id],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        if seen {
            continue;
        }
        db.execute(
            "INSERT OR IGNORE INTO phone_receipts(id,ask_id,token,received_ms) VALUES(?1,?2,?3,?4)",
            params![message.id, ask_id, message.token.0, now],
        )
        .map_err(db_err)?;
        events::append(
            db,
            now,
            "phone.received",
            &ask_id,
            &json!({"id":message.id}),
        )
        .map_err(db_err)?;
    }
    if let Some(cursor) = cursor {
        if !identifier(&cursor) {
            return Err("invalid phone reply cursor".into());
        }
        store::set_meta(db, "phone_cursor", &cursor).map_err(db_err)?;
    }
    Ok(())
}

pub fn complete(db: &Connection, now: i64, id: &str, outcome: &str) -> Result<(), String> {
    db.execute("UPDATE phone_receipts SET completed_ms=?1,outcome=?2,token='' WHERE id=?3 AND completed_ms IS NULL", params![now,outcome,id]).map_err(db_err)?;
    Ok(())
}
pub fn receipt(db: &Connection, id: &str) -> Result<Option<(String, String, Option<i64>)>, String> {
    db.query_row(
        "SELECT ask_id,token,completed_ms FROM phone_receipts WHERE id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .optional()
    .map_err(db_err)
}

pub fn work(
    db: &Connection,
    now: i64,
    secret: &[u8],
    generation: &str,
    op: Work,
) -> Result<ResultValue, String> {
    let cfg = config(db)?;
    let mut health: Health = read(db, "phone_health")?;
    if matches!(op, Work::State) {
        return Ok(ResultValue::State(State {
            config: cfg,
            health,
            cursor: store::meta(db, "phone_cursor").map_err(db_err)?,
        }));
    }
    if !cfg.settings.enabled || cfg.generation != generation {
        return Err("phone settings changed".into());
    }
    match op {
        Work::State => unreachable!(),
        Work::Verified => {
            health.protected_at = Some(now);
            health.error = None;
        }
        Work::Fault(message) => {
            health.error = Some(rules::clean_line(&message, 300));
        }
        Work::Prepare => {
            if !health
                .protected_at
                .is_some_and(|at| now.saturating_sub(at) < PROTECTION_TTL_MS)
            {
                return Err("phone topic protection has not been verified".into());
            }
            return prepare(db, now, secret).map(ResultValue::Notification);
        }
        Work::Published(toast_id) => {
            db.execute(
                "UPDATE phone_outbox SET sent_ms=?1 WHERE toast_id=?2 AND sent_ms IS NULL",
                params![now, toast_id],
            )
            .map_err(db_err)?;
            health.published_at = Some(now);
            health.error = None;
        }
        Work::Receive { messages, cursor } => {
            receive(db, now, secret, messages, cursor)?;
            health.polled_at = Some(now);
            health.error = None;
        }
        Work::Pending => {
            let mut statement=db.prepare("SELECT id FROM phone_receipts WHERE completed_ms IS NULL ORDER BY received_ms LIMIT 20").map_err(db_err)?;
            let ids = statement
                .query_map([], |r| r.get(0))
                .map_err(db_err)?
                .collect::<Result<Vec<String>, _>>()
                .map_err(db_err)?;
            let mut statement=db.prepare("SELECT id,ask_id,outcome FROM phone_receipts WHERE completed_ms IS NOT NULL AND acknowledged_ms IS NULL ORDER BY received_ms LIMIT 20").map_err(db_err)?;
            let acknowledgements = statement
                .query_map([], |r| {
                    Ok(Acknowledgement {
                        id: r.get(0)?,
                        ask_id: r.get(1)?,
                        outcome: r.get(2)?,
                    })
                })
                .map_err(db_err)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(db_err)?;
            return Ok(ResultValue::Pending {
                ids,
                acknowledgements,
            });
        }
        Work::Acknowledged(id) => {
            db.execute("UPDATE phone_receipts SET acknowledged_ms=?1 WHERE id=?2 AND completed_ms IS NOT NULL",params![now,id]).map_err(db_err)?;
            // Retain recent receipt dedupe and remove old, fully acknowledged rows.
            db.execute(
                "DELETE FROM phone_receipts WHERE acknowledged_ms<?1",
                [now - tokens::TOKEN_TTL_MS],
            )
            .map_err(db_err)?;
        }
    }
    store::set_meta(db, "phone_health", &json!(health).to_string()).map_err(db_err)?;
    Ok(ResultValue::Done)
}

pub fn time_label(ms: Option<i64>) -> String {
    ms.map(fmt_ms).unwrap_or_else(|| "Not yet".into())
}
