//! `StoreService`: the store-backed `Service` (PLAN.md "Rules for asks and work", "What an answer means",
//! "The Desk"). Every operation runs in one SQLite transaction and appends its audit events in it; effects
//! (tray count, toasts, `codex queue`) are returned only after the commit.
//!
use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde_json::{Value, json};
use time::OffsetDateTime;

use crate::model::{Agent, Answer, AnswerVia, Ask, AskInput, AskStatus, Owner, Priority, Settings};
use crate::protocol::{
    AgentsResult, AnswerResult, AnswersResult, AskResult, DeskOp, DigestResult, ListResult, McpOp,
    OwnersResult, RegisterResult, RelayListResult, StatusResult,
};
use crate::service::{ClientInfo, Effect, Service};
use crate::store::{self, AskRow, db_err, fmt_ms, status_str};
use crate::{events, rules, tokens};

/// The source of "now" for operations. Tests inject a fixed or stepping clock.
pub type Clock = Arc<dyn Fn() -> OffsetDateTime + Send + Sync>;

pub const UNKNOWN_HANDLE: &str = "unknown handle; call desk_register first";
/// Handles not seen for this long are forgotten (a later call with one gets `UNKNOWN_HANDLE`).
pub const HANDLE_RETENTION_MS: i64 = 7 * DAY_MS;
/// An agent counts as alive in `StatusResult.agents` when seen within this long (the 4-hour ceiling).
pub const ALIVE_MS: i64 = 4 * 60 * 60 * 1000;
/// `Answers` with no ids and `Register` list undelivered answers from this far back.
pub const ANSWER_WINDOW_MS: i64 = 14 * DAY_MS;
const ANSWER_LIST_LIMIT: i64 = 50;
const RELAY_LIST_LIMIT: i64 = 100;
const RECENT_LIMIT: i64 = 20;
const MAX_IDS: usize = 100;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;
/// `last_seen` is rewritten at most this often, so a polling client doesn't write on every poll.
const TOUCH_EVERY_MS: i64 = 1000;

type Snapshot = (u32, u32, Option<String>);

pub struct StoreService {
    conn: Mutex<Connection>,
    clock: Clock,
    channel: String,
    machine: String,
    secret: [u8; tokens::SECRET_LEN],
    last_inbox: Mutex<Option<Snapshot>>,
    /// How far Status has verified the event chain, so a Desk polling Status doesn't re-walk the whole log.
    chain_mark: Mutex<ChainMark>,
}

/// The last verified event, and when the whole chain was last walked.
#[derive(Default)]
struct ChainMark {
    seq: i64,
    hash: Option<String>,
    full_at_ms: Option<i64>,
}

/// Status walks the whole chain at most this often; in between it checks only new rows and the mark row.
const FULL_CHAIN_EVERY_MS: i64 = 10 * 60_000;

impl StoreService {
    /// Open the daemon's database: creates parent folders, migrates, sets WAL and exclusive locking (the daemon
    /// is the only writer; a second open of the same file fails), and makes the toast secret on first open.
    pub fn open(db_path: &Path) -> Result<StoreService, String> {
        Self::from_conn(store::open_file(db_path)?)
    }

    /// An in-memory store (tests).
    pub fn open_in_memory() -> Result<StoreService, String> {
        Self::from_conn(store::open_memory()?)
    }

    fn from_conn(mut conn: Connection) -> Result<StoreService, String> {
        let clock: Clock = Arc::new(OffsetDateTime::now_utc);
        let secret = {
            let tx = conn.transaction().map_err(db_err)?;
            let now = store::to_ms(clock());
            let legacy = store::ask_rows(&tx, "status = 'digest'", []).map_err(db_err)?;
            for row in legacy {
                tx.execute("UPDATE asks SET status = 'open', inbox_ms = COALESCE(inbox_ms, ?1) WHERE id = ?2", params![now, row.id]).map_err(db_err)?;
                events::append(
                    &tx,
                    now,
                    "ask.migrated_to_inbox",
                    &row.id,
                    &json!({"owner":row.owner}),
                )
                .map_err(db_err)?;
            }
            let stored = store::meta(&tx, "toast_secret").map_err(db_err)?;
            let secret = match stored
                .and_then(|h| hex::decode(h).ok())
                .filter(|b| b.len() == tokens::SECRET_LEN)
            {
                Some(bytes) => {
                    let mut s = [0u8; tokens::SECRET_LEN];
                    s.copy_from_slice(&bytes);
                    s
                }
                None => {
                    let s = tokens::new_secret();
                    store::set_meta(&tx, "toast_secret", &hex::encode(s)).map_err(db_err)?;
                    events::append(
                        &tx,
                        store::to_ms(clock()),
                        "secret.created",
                        "toast_secret",
                        &json!({}),
                    )
                    .map_err(db_err)?;
                    s
                }
            };
            tx.commit().map_err(db_err)?;
            secret
        };
        let channel = crate::channel::Names::from_env()
            .map(|n| n.channel)
            .unwrap_or_else(|_| "main".into());
        let machine = ["COMPUTERNAME", "HOSTNAME"]
            .iter()
            .find_map(|k| std::env::var(k).ok().filter(|v| !v.trim().is_empty()))
            .unwrap_or_else(|| "local".into());
        Ok(StoreService {
            conn: Mutex::new(conn),
            clock,
            channel,
            machine,
            secret,
            last_inbox: Mutex::new(None),
            chain_mark: Mutex::new(ChainMark::default()),
        })
    }

    /// Replace the clock operations use (tests; `tick` takes its own `now`).
    pub fn with_clock(mut self, clock: Clock) -> Self {
        self.clock = clock;
        self
    }

    /// The channel name reported by `Status` (default: `AGENTBOLT_CHANNEL`, else `main`).
    pub fn with_channel(mut self, channel: impl Into<String>) -> Self {
        self.channel = channel.into();
        self
    }

    /// The machine name stored on agents (default: `COMPUTERNAME` / `HOSTNAME`).
    pub fn with_machine(mut self, machine: impl Into<String>) -> Self {
        self.machine = machine.into();
        self
    }

    /// Record a delivery the daemon made (e.g. after `codex queue`). `to` is where it went: for Codex, the
    /// thread. Any successful delivery takes the answer off the undelivered lists.
    pub fn record_delivery(
        &self,
        ask_id: &str,
        to: &str,
        ok: bool,
        detail: Option<&str>,
    ) -> Result<(), String> {
        let now = self.now_ms();
        self.write(now, |c| c.deliver(ask_id, to, ok, detail, "daemon"))
            .map(|_| ())
    }

    /// Recheck immediately before queued delivery: an owner may have revoked wake permission while waiting.
    pub fn codex_delivery_allowed(&self, ask_id: &str, thread: &str) -> bool {
        let conn = self.lock();
        let Ok(Some(row)) = store::ask_row(&conn, ask_id) else {
            return false;
        };
        is_answered(row.status)
            && row.asking_session.as_deref() == Some(thread)
            && is_codex_chat(&row)
            && store::owner(&conn, &row.owner).is_ok_and(|o| o.is_some_and(|o| o.codex_interactive))
    }

    /// Mint fresh toast buttons (label, token) for an open or digest ask that may have them (reversible,
    /// choose/approve, up to 5 options); None otherwise. `Effect::Toast` already carries buttons for new asks;
    /// this is for showing a toast again.
    pub fn mint_toast(&self, ask_id: &str) -> Option<Vec<(String, String)>> {
        let now = self.now_ms();
        self.write(now, |c| {
            let row = c.row(ask_id)?;
            match row {
                Some(r) if is_pending(r.status) && rules::toast_buttons_allowed(&r.input) => {
                    Ok(Some(c.mint_buttons(&r)?))
                }
                _ => Ok(None),
            }
        })
        .ok()
        .and_then(|(v, _)| v)
    }

    /// Verify the event log's hash chain: Ok(number of events) or the first broken row.
    pub fn verify_chain(&self) -> Result<u64, String> {
        events::verify_chain(&self.lock())
    }

    #[cfg(test)]
    pub(crate) fn conn_for_tests(&self) -> MutexGuard<'_, Connection> {
        self.lock()
    }

    fn now_ms(&self) -> i64 {
        store::to_ms((self.clock)())
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Run `f` in one transaction; on Ok commit, then add `InboxChanged` if the counts changed.
    fn write<T>(
        &self,
        now: i64,
        f: impl FnOnce(&mut Ctx<'_>) -> Result<T, String>,
    ) -> Result<(T, Vec<Effect>), String> {
        let mut conn = self.lock();
        let tx = conn.transaction().map_err(db_err)?;
        let (out, mut effects) = {
            let mut ctx = Ctx {
                db: &tx,
                now,
                secret: &self.secret,
                effects: Vec::new(),
            };
            let out = f(&mut ctx)?;
            (out, ctx.effects)
        };
        tx.commit().map_err(db_err)?;
        if let Some(e) = self.inbox_effect(&conn).map_err(db_err)? {
            effects.insert(0, e);
        }
        Ok((out, effects))
    }

    fn write_json<T: Serialize>(
        &self,
        now: i64,
        f: impl FnOnce(&mut Ctx<'_>) -> Result<T, String>,
    ) -> Result<(Value, Vec<Effect>), String> {
        let (out, effects) = self.write(now, f)?;
        Ok((to_json(&out)?, effects))
    }

    fn inbox_effect(&self, conn: &Connection) -> rusqlite::Result<Option<Effect>> {
        let open = store::count(conn, "SELECT COUNT(*) FROM asks WHERE status = 'open'", [])?;
        let digest = store::count(
            conn,
            "SELECT COUNT(*) FROM asks WHERE status = 'digest'",
            [],
        )?;
        let next: Option<i64> = conn.query_row(
            "SELECT MIN(deadline_ms) FROM asks WHERE status = 'open'",
            [],
            |r| r.get(0),
        )?;
        let snap = (open, digest, next.map(fmt_ms));
        let mut last = self.last_inbox.lock().unwrap_or_else(|p| p.into_inner());
        if last.as_ref() == Some(&snap) {
            return Ok(None);
        }
        *last = Some(snap.clone());
        Ok(Some(Effect::InboxChanged {
            open: snap.0,
            digest: snap.1,
            next_deadline: snap.2,
        }))
    }

    /// The event chain's state for Status: a full walk every `FULL_CHAIN_EVERY_MS`, otherwise only the rows after
    /// the last verified one, plus a check that the mark row itself is unchanged.
    fn chain_ok(&self, conn: &Connection, now: i64) -> bool {
        let mut mark = self.chain_mark.lock().unwrap_or_else(|e| e.into_inner());
        let full_due = mark
            .full_at_ms
            .is_none_or(|at| now - at >= FULL_CHAIN_EVERY_MS);
        let result = if full_due || mark.hash.is_none() {
            events::verify_chain_from(conn, 0, events::GENESIS).map(|r| (r, true))
        } else {
            let prev = mark.hash.clone().unwrap_or_default();
            // Rows before the mark can only change if the append-only triggers were dropped, so check they're there.
            let guarded = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger' AND name IN ('events_no_update', 'events_no_delete')",
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .is_ok_and(|n| n == 2);
            let unchanged = conn
                .query_row("SELECT hash FROM events WHERE seq = ?1", [mark.seq], |r| {
                    r.get::<_, String>(0)
                })
                .is_ok_and(|h| h == prev);
            if !guarded {
                events::verify_chain_from(conn, 0, events::GENESIS).map(|r| (r, true))
            } else if unchanged {
                events::verify_chain_from(conn, mark.seq, &prev).map(|r| (r, false))
            } else {
                Err("the last verified event changed".to_string())
            }
        };
        match result {
            Ok(((seq, hash), full)) => {
                mark.seq = seq;
                mark.hash = Some(hash);
                if full {
                    mark.full_at_ms = Some(now);
                }
                true
            }
            Err(_) => {
                *mark = ChainMark::default();
                false
            }
        }
    }

    fn status(&self, now: i64) -> Result<(Value, Vec<Effect>), String> {
        let conn = self.lock();
        let open = store::count(&conn, "SELECT COUNT(*) FROM asks WHERE status = 'open'", [])
            .map_err(db_err)?;
        let digest = store::count(
            &conn,
            "SELECT COUNT(*) FROM asks WHERE status = 'digest'",
            [],
        )
        .map_err(db_err)?;
        let agents = store::count(
            &conn,
            "SELECT COUNT(*) FROM agents WHERE last_seen_ms >= ?1",
            [now - ALIVE_MS],
        )
        .map_err(db_err)?;
        let chain_ok = self.chain_ok(&conn, now);
        let r = StatusResult {
            version: env!("CARGO_PKG_VERSION").to_string(),
            channel: self.channel.clone(),
            open,
            digest,
            agents,
            chain_ok,
        };
        Ok((to_json(&r)?, Vec::new()))
    }
}

fn to_json<T: Serialize>(v: &T) -> Result<Value, String> {
    serde_json::to_value(v).map_err(|e| format!("can't encode the result: {e}"))
}

fn is_pending(s: AskStatus) -> bool {
    matches!(s, AskStatus::Open | AskStatus::Digest)
}

fn is_answered(s: AskStatus) -> bool {
    matches!(s, AskStatus::Answered | AskStatus::Defaulted)
}

fn is_codex_chat(row: &AskRow) -> bool {
    matches!(
        crate::navigation::ChatTarget::from_host(
            row.asking_client.as_deref().unwrap_or_default(),
            row.asking_session.as_deref()
        ),
        Some(crate::navigation::ChatTarget::Codex(_))
    )
}

fn opt_line(s: Option<String>, max: usize, what: &str) -> Result<Option<String>, String> {
    match s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty()) {
        None => Ok(None),
        Some(v) if v.chars().count() > max => Err(format!("{what} takes at most {max} characters")),
        Some(v) if v.chars().any(char::is_control) => {
            Err(format!("{what} can't contain control characters"))
        }
        Some(v) => Ok(Some(v)),
    }
}

/// For the asking owner's own lists (`desk_register`, `desk_answers` without ids): not yet delivered to the session
/// that asked (or to the owner, when no session is known). A relay's "notes" delivery doesn't count here: the
/// agent that asked may not read NOTES.md, so it must still find its answer.
const UNDELIVERED_TO_ASKER: &str = "NOT EXISTS (SELECT 1 FROM deliveries d WHERE d.ask_id = asks.id AND d.ok = 1
                           AND (d.target = asks.asking_session OR (asks.asking_session IS NULL AND d.target = asks.owner)))";

/// For the relay's list: no successful delivery recorded yet, wherever it went. The relay may only be able to
/// write an answer to NOTES.md (`to` = "notes"), and that counts here, or such answers would sit on its list forever.
const UNDELIVERED_ANYWHERE: &str =
    "NOT EXISTS (SELECT 1 FROM deliveries d WHERE d.ask_id = asks.id AND d.ok = 1)";

/// The fields of `McpOp::Register`.
struct Registration {
    owner: Option<String>,
    client: String,
    session_ref: Option<String>,
    cwd: Option<String>,
    doing: Option<String>,
    provisional_key: Option<String>,
}

/// One transaction's view: the connection, "now", the toast secret, and the effects collected so far.
struct Ctx<'a> {
    db: &'a Connection,
    now: i64,
    secret: &'a [u8],
    effects: Vec<Effect>,
}

impl Ctx<'_> {
    fn event(&self, kind: &str, subject: &str, data: Value) -> Result<(), String> {
        events::append(self.db, self.now, kind, subject, &data)
            .map(|_| ())
            .map_err(db_err)
    }

    fn row(&self, id: &str) -> Result<Option<AskRow>, String> {
        store::ask_row(self.db, id).map_err(db_err)
    }

    fn full(&self, row: &AskRow) -> Result<Ask, String> {
        Ok(Ask {
            id: row.id.clone(),
            owner: row.owner.clone(),
            muted: row.muted,
            asking_session: row.asking_session.clone(),
            asking_client: row.asking_client.clone(),
            asking_cwd: row.asking_cwd.clone(),
            input: row.input.clone(),
            status: row.status,
            created_at: fmt_ms(row.created_ms),
            shown_at: row.shown_ms.map(fmt_ms),
            answer: store::answer_for(self.db, &row.id).map_err(db_err)?,
        })
    }

    fn full_all(&self, rows: &[AskRow]) -> Result<Vec<Ask>, String> {
        rows.iter().map(|r| self.full(r)).collect()
    }

    fn settings(&self) -> Result<Settings, String> {
        store::settings(self.db).map_err(db_err)
    }

    /// Check a handle and renew its lease (rule 5: any call made with a handle renews it).
    fn touch(&self, handle: &str, client: &ClientInfo) -> Result<Agent, String> {
        let row: Option<(i64, Option<i64>)> = self
            .db
            .query_row(
                "SELECT last_seen_ms, connection FROM agents WHERE handle = ?1",
                [handle],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db_err)?;
        let Some((last_seen, connection)) = row else {
            return Err(UNKNOWN_HANDLE.into());
        };
        let conn_id = client.connection as i64;
        if self.now - last_seen >= TOUCH_EVERY_MS || connection != Some(conn_id) {
            self.db
                .execute(
                    "UPDATE agents SET last_seen_ms = MAX(last_seen_ms, ?1), connection = ?2 WHERE handle = ?3",
                    params![self.now, conn_id, handle],
                )
                .map_err(db_err)?;
        }
        store::agent(self.db, handle)
            .map_err(db_err)?
            .ok_or_else(|| UNKNOWN_HANDLE.into())
    }

    /// This owner's asks still open or in the digest, oldest first.
    fn pending_of(&self, owner: &str) -> Result<Vec<AskRow>, String> {
        store::ask_rows(
            self.db,
            "owner = ?1 AND status IN ('open', 'digest') ORDER BY created_ms, rowid",
            [owner],
        )
        .map_err(db_err)
    }

    fn undelivered_of(&self, owner: &str) -> Result<Vec<Ask>, String> {
        let rows = store::ask_rows(
            self.db,
            &format!(
                "owner = ?1 AND status IN ('answered', 'defaulted') AND closed_ms >= ?2 AND {UNDELIVERED_TO_ASKER}
                 ORDER BY closed_ms DESC, rowid DESC LIMIT {ANSWER_LIST_LIMIT}"
            ),
            params![owner, self.now - ANSWER_WINDOW_MS],
        )
        .map_err(db_err)?;
        self.full_all(&rows)
    }

    // ---- toasts ----

    fn maybe_toast(&mut self, row: &AskRow, settings: &Settings) -> Result<(), String> {
        if row.muted
            || settings.toasts_off
            || !rules::toast_worthy(&row.input, row.deadline_ms, self.now)
        {
            return Ok(());
        }
        let notified: bool = self
            .db
            .query_row(
                "SELECT toast_notified FROM asks WHERE id = ?1",
                [&row.id],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        if notified {
            return Ok(());
        }
        let (last, head): (Option<i64>, Option<String>) = self
            .db
            .query_row(
                "SELECT last_toast_ms, toast_head FROM owners WHERE name = ?1",
                [&row.owner],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(db_err)?;
        let suppress_popup =
            last.is_some_and(|at| self.now < at.saturating_add(rules::TOAST_INTERVAL_MS));
        let head = if suppress_popup {
            head.as_deref()
                .map(|id| self.row(id))
                .transpose()?
                .flatten()
                .filter(|r| {
                    r.status == AskStatus::Open
                        && r.owner == row.owner
                        && rules::toast_worthy(&r.input, r.deadline_ms, self.now)
                })
        } else {
            None
        };
        let head = head.as_ref().unwrap_or(row);
        let count = store::count(self.db,
            "SELECT COUNT(*) FROM asks WHERE owner = ?1 AND status = 'open' AND (priority <> 'P2' OR deadline_ms <= ?2)",
            params![row.owner, self.now + rules::URGENT_MS]).map_err(db_err)?;
        self.db.execute(
            "UPDATE asks SET toast_notified = 1 WHERE owner = ?1 AND status = 'open' AND (priority <> 'P2' OR deadline_ms <= ?2)",
            params![row.owner, self.now + rules::URGENT_MS]).map_err(db_err)?;
        self.db.execute(
            "UPDATE owners SET last_toast_ms = CASE WHEN ?1 THEN last_toast_ms ELSE ?2 END, toast_head = ?3 WHERE name = ?4",
            params![suppress_popup, self.now, head.id, row.owner]).map_err(db_err)?;
        let buttons = if rules::toast_buttons_allowed(&head.input) {
            self.mint_buttons(head)?
        } else {
            Vec::new()
        };
        let more = count.saturating_sub(1);
        let body = rules::toast_body(&head.owner, &head.input.body);
        let body = if more > 0 {
            format!("{more} more waiting. {body}")
        } else {
            body
        };
        use sha2::Digest;
        let tag = hex::encode(&sha2::Sha256::digest(row.owner.as_bytes())[..8]);
        self.event(
            "toast.batched",
            &head.id,
            json!({"owner":row.owner,"more":more,"suppress_popup":suppress_popup}),
        )?;
        self.effects.push(Effect::Toast {
            ask_id: head.id.clone(),
            tag,
            suppress_popup,
            title: head.input.title.clone(),
            body,
            buttons,
        });
        Ok(())
    }

    /// P2 asks become eligible when their deadlines approach, even with no new MCP traffic.
    fn due_toasts(&mut self) -> Result<(), String> {
        let settings = self.settings()?;
        if settings.toasts_off {
            return Ok(());
        }
        let rows = store::ask_rows(self.db,
            "status = 'open' AND toast_notified = 0 AND muted = 0 AND (priority <> 'P2' OR deadline_ms <= ?1)
             ORDER BY priority, deadline_ms, created_ms, rowid", [self.now + rules::URGENT_MS]).map_err(db_err)?;
        for row in rows {
            self.maybe_toast(&row, &settings)?;
        }
        Ok(())
    }

    fn mint_buttons(&self, row: &AskRow) -> Result<Vec<(String, String)>, String> {
        let toast_id = tokens::new_toast_id();
        self.db
            .execute(
                "INSERT INTO toasts (toast_id, ask_id, created_ms, expires_ms) VALUES (?1, ?2, ?3, ?4)",
                params![toast_id, row.id, self.now, self.now + tokens::TOKEN_TTL_MS],
            )
            .map_err(db_err)?;
        self.event(
            "toast.minted",
            &row.id,
            json!({ "toast": toast_id, "buttons": row.input.options.len() }),
        )?;
        Ok(row
            .input
            .options
            .iter()
            .enumerate()
            .map(|(i, label)| {
                (
                    label.clone(),
                    tokens::mint(self.secret, &row.id, i, &toast_id),
                )
            })
            .collect())
    }

    fn revoke_toasts(&self, ask_id: &str) -> Result<(), String> {
        self.db
            .execute(
                "UPDATE toasts SET revoked_ms = ?1 WHERE ask_id = ?2 AND used_ms IS NULL AND revoked_ms IS NULL",
                params![self.now, ask_id],
            )
            .map_err(db_err)?;
        Ok(())
    }

    // ---- answers ----

    fn write_answer(
        &mut self,
        row: &AskRow,
        via: AnswerVia,
        choice: Option<String>,
        values: Option<Value>,
        note: Option<String>,
    ) -> Result<Answer, String> {
        let answer = Answer {
            ask_id: row.id.clone(),
            via,
            choice,
            values,
            note,
            answered_at: fmt_ms(self.now),
            deliveries: Vec::new(),
        };
        store::insert_answer(self.db, &answer, self.now).map_err(db_err)?;
        let (status, kind) = match via {
            AnswerVia::Defaulted => (AskStatus::Defaulted, "ask.defaulted"),
            AnswerVia::Desk | AnswerVia::Toast => (AskStatus::Answered, "ask.answered"),
            AnswerVia::NotNeeded => (AskStatus::Answered, "ask.not_needed"),
        };
        // A default can only happen to a shown ask; a clicked answer means it has been seen now.
        self.db
            .execute(
                "UPDATE asks SET status = ?1, closed_ms = ?2, shown_ms = COALESCE(shown_ms, ?2) WHERE id = ?3",
                params![status_str(status), self.now, row.id],
            )
            .map_err(db_err)?;
        self.revoke_toasts(&row.id)?;
        self.event(
            kind,
            &row.id,
            json!({
                "owner": row.owner,
                "via": store::via_str(via),
                "choice": answer.choice,
                "values": answer.values,
                "note": answer.note,
            }),
        )?;
        let owner = store::owner(self.db, &row.owner).map_err(db_err)?;
        if let (Some(o), Some(thread)) = (owner, &row.asking_session)
            && o.codex_interactive
            && is_codex_chat(row)
        {
            self.effects.push(Effect::CodexQueue {
                ask_id: row.id.clone(),
                thread: thread.clone(),
                text: rules::codex_text(&row.id, &row.input.title, &answer),
            });
        }
        Ok(answer)
    }

    /// Rule: a deadline default applies only to a reversible ask Artem has been shown, and no sooner than
    /// `rules::DEFAULT_GRACE_MS` after he was first shown it (`rules::default_due_ms`). Irreversible asks and
    /// unseen asks stay open, overdue.
    fn apply_defaults(&mut self) -> Result<(), String> {
        let due = store::ask_rows(
            self.db,
            "status IN ('open', 'digest') AND reversible = 1 AND shown_ms IS NOT NULL AND deadline_ms <= ?1
             ORDER BY deadline_ms, rowid",
            [self.now],
        )
        .map_err(db_err)?;
        let now = self.now;
        let due = due.into_iter().filter(|r| {
            r.shown_ms
                .is_some_and(|s| rules::default_due_ms(r.deadline_ms, s) <= now)
        });
        for row in due {
            let (choice, values) = rules::default_answer(&row.input);
            self.write_answer(&row, AnswerVia::Defaulted, choice, values, None)?;
        }
        Ok(())
    }

    fn deliver(
        &self,
        ask_id: &str,
        to: &str,
        ok: bool,
        detail: Option<&str>,
        by: &str,
    ) -> Result<Answer, String> {
        let to = to.trim();
        if to.is_empty() || to.chars().count() > 300 {
            return Err("a delivery needs `to`: where it went (1-300 characters)".into());
        }
        let detail = detail
            .map(|d| rules::clean_line(d, 2000))
            .filter(|d| !d.is_empty());
        let row = self
            .row(ask_id)?
            .ok_or_else(|| format!("no ask {ask_id}"))?;
        if !is_answered(row.status) {
            return Err(format!("ask {ask_id} has no answer to deliver"));
        }
        store::insert_delivery(self.db, ask_id, to, self.now, ok, detail.as_deref())
            .map_err(db_err)?;
        self.event(
            "delivery",
            ask_id,
            json!({ "to": to, "ok": ok, "detail": detail, "by": by }),
        )?;
        store::answer_for(self.db, ask_id)
            .map_err(db_err)?
            .ok_or_else(|| format!("ask {ask_id} has no answer"))
    }

    // ---- MCP operations ----

    fn register(
        &mut self,
        client: &ClientInfo,
        machine: &str,
        r: Registration,
    ) -> Result<RegisterResult, String> {
        let Registration {
            owner,
            client: client_name,
            session_ref,
            cwd,
            doing,
            provisional_key,
        } = r;
        let client_name = rules::clean_line(&client_name, 100);
        if client_name.is_empty() {
            return Err("desk_register needs `client`, such as claude-code or codex".into());
        }
        let session_ref = opt_line(session_ref, 200, "session_ref")?;
        let provisional_key = opt_line(provisional_key, 200, "provisional_key")?;
        let cwd = opt_line(cwd, 1024, "cwd")?;
        let doing = doing
            .map(|d| rules::clean_line(&d, 200))
            .filter(|d| !d.is_empty());
        let owner_name = match owner
            .map(|o| o.trim().to_string())
            .filter(|o| !o.is_empty())
        {
            Some(name) => {
                rules::check_owner_name(&name)?;
                self.bind_owner(&name, "config")?;
                name
            }
            None => {
                // Provisional owners are per host session, never per folder: every Compass routine shares one
                // cwd. With no host session, one per MCP process (its key is made once at process start).
                let id = session_ref
                    .clone()
                    .or(provisional_key)
                    .unwrap_or_else(|| tokens::random_hex(8));
                let name = format!("session:{id}");
                self.bind_owner(&name, "pending")?;
                name
            }
        };
        let handle = format!("h{}", tokens::random_hex(16));
        store::insert_agent(
            self.db,
            &store::NewAgent {
                handle: &handle,
                owner: &owner_name,
                client: &client_name,
                session_ref: session_ref.as_deref(),
                cwd: cwd.as_deref(),
                machine,
                doing: doing.as_deref(),
                now_ms: self.now,
                connection: client.connection as i64,
            },
        )
        .map_err(db_err)?;
        let agent = store::agent(self.db, &handle)
            .map_err(db_err)?
            .ok_or("the new handle vanished")?;
        self.event(
            "agent.registered",
            &owner_name,
            json!({ "client": client_name, "session_ref": session_ref, "cwd": cwd, "provisional": agent.provisional }),
        )?;
        Ok(RegisterResult {
            handle,
            owner: owner_name.clone(),
            provisional: agent.provisional,
            answers: self.undelivered_of(&owner_name)?,
            open_asks: self.full_all(&self.pending_of(&owner_name)?)?,
        })
    }

    /// Record an owner the first time it appears. Config binding upgrades a pending owner; an existing binding
    /// (config or approved) is kept.
    fn bind_owner(&self, name: &str, binding: &str) -> Result<Owner, String> {
        match store::owner(self.db, name).map_err(db_err)? {
            Some(mut o) => {
                if binding == "config" && o.binding == "pending" {
                    o.binding = "config".into();
                    store::upsert_owner(self.db, &o, self.now).map_err(db_err)?;
                    self.event("owner.bound", name, json!({ "binding": "config" }))?;
                }
                Ok(o)
            }
            None => {
                let o = Owner {
                    name: name.into(),
                    binding: binding.into(),
                    codex_interactive: false,
                    relay: false,
                    muted: false,
                    not_needed_count: 0,
                };
                store::upsert_owner(self.db, &o, self.now).map_err(db_err)?;
                self.event("owner.bound", name, json!({ "binding": binding }))?;
                Ok(o)
            }
        }
    }

    fn ask(
        &mut self,
        client: &ClientInfo,
        handle: &str,
        input: &AskInput,
    ) -> Result<AskResult, String> {
        let agent = self.touch(handle, client)?;
        let (mut input, deadline_ms) = rules::validate_ask(input, self.now)?;

        let pending = self.pending_of(&agent.owner)?;
        if let Some(dup) = pending
            .iter()
            .find(|r| rules::is_duplicate(&r.input, &input))
        {
            return Ok(AskResult {
                id: dup.id.clone(),
                placed: "existing".into(),
                existing: Some(self.full(dup)?),
                note: None,
            });
        }
        if pending.len() >= rules::OWNER_CAP {
            let oldest = &pending[0];
            return Ok(AskResult {
                id: oldest.id.clone(),
                placed: "cap".into(),
                existing: Some(self.full(oldest)?),
                note: None,
            });
        }

        let settings = self.settings()?;
        let mut demoted = None;
        if input.priority == Priority::P0 && agent.provisional {
            input.priority = Priority::P1;
            demoted = Some(
                "Stored as P1: P0 requires an owner bound by the server's --owner or approved on the Desk.",
            );
        }
        let status = AskStatus::Open;
        let id = format!("ask_{}", tokens::random_hex(8));
        store::insert_ask(
            self.db,
            &store::NewAsk {
                id: &id,
                owner: &agent.owner,
                asking_session: agent.session_ref.as_deref(),
                asking_client: Some(&agent.client),
                asking_cwd: agent.cwd.as_deref(),
                input: &input,
                p0_skip: false,
                deadline_ms,
                status,
                created_ms: self.now,
                inbox_ms: (status == AskStatus::Open).then_some(self.now),
            },
        )
        .map_err(db_err)?;
        self.event(
            "ask.created",
            &id,
            json!({
                "owner": agent.owner,
                "placed": status_str(status),
                "demoted_from_p0": demoted.is_some(),
                "note": demoted,
                "provisional": agent.provisional,
                "input": input,
            }),
        )?;
        if status == AskStatus::Open {
            let row = self.row(&id)?.ok_or("the new ask vanished")?;
            self.maybe_toast(&row, &settings)?;
        }
        Ok(AskResult {
            id,
            placed: status_str(status).into(),
            existing: None,
            note: demoted.map(String::from),
        })
    }

    fn answers(
        &self,
        client: &ClientInfo,
        handle: &str,
        ids: &[String],
    ) -> Result<AnswersResult, String> {
        let agent = self.touch(handle, client)?;
        let mut r = AnswersResult::default();
        if ids.is_empty() {
            r.answers = self.undelivered_of(&agent.owner)?;
            r.waiting = self
                .pending_of(&agent.owner)?
                .into_iter()
                .map(|a| a.id)
                .collect();
            return Ok(r);
        }
        let mut seen = HashSet::new();
        for id in ids
            .iter()
            .map(|i| i.trim())
            .filter(|i| !i.is_empty())
            .take(MAX_IDS)
        {
            if !seen.insert(id.to_string()) {
                continue;
            }
            match self.row(id)? {
                Some(row) if row.owner == agent.owner => match row.status {
                    AskStatus::Answered | AskStatus::Defaulted => r.answers.push(self.full(&row)?),
                    AskStatus::Open | AskStatus::Digest => r.waiting.push(row.id),
                    AskStatus::Cancelled => r.cancelled.push(row.id),
                },
                _ => r.unknown.push(id.to_string()),
            }
        }
        Ok(r)
    }

    fn cancel(&mut self, client: &ClientInfo, handle: &str, id: &str) -> Result<Ask, String> {
        let agent = self.touch(handle, client)?;
        let row = self
            .row(id)?
            .filter(|r| r.owner == agent.owner)
            .ok_or_else(|| format!("no ask {id} for owner {}", agent.owner))?;
        if !is_pending(row.status) {
            return Err(format!("ask {id} is already {}", status_str(row.status)));
        }
        self.db
            .execute(
                "UPDATE asks SET status = 'cancelled', closed_ms = ?1 WHERE id = ?2",
                params![self.now, id],
            )
            .map_err(db_err)?;
        self.revoke_toasts(id)?;
        self.event("ask.cancelled", id, json!({ "owner": agent.owner }))?;
        let row = self.row(id)?.ok_or("the cancelled ask vanished")?;
        self.full(&row)
    }

    fn relay_agent(&self, client: &ClientInfo, handle: &str) -> Result<Agent, String> {
        let agent = self.touch(handle, client)?;
        let relay = store::owner(self.db, &agent.owner)
            .map_err(db_err)?
            .is_some_and(|o| o.relay);
        if !relay {
            return Err(format!(
                "desk_relay is only for the relay owner; {} isn't one (Artem sets it on the Desk)",
                agent.owner
            ));
        }
        Ok(agent)
    }

    fn relay_list(&self, client: &ClientInfo, handle: &str) -> Result<RelayListResult, String> {
        let agent = self.relay_agent(client, handle)?;
        // The same 14-day window as an owner's own list, newest first, so old answers can't crowd out new ones.
        let rows = store::ask_rows(
            self.db,
            &format!(
                "owner <> ?1 AND status IN ('answered', 'defaulted') AND closed_ms >= ?2 AND {UNDELIVERED_ANYWHERE}
                 ORDER BY closed_ms DESC, rowid DESC LIMIT {RELAY_LIST_LIMIT}"
            ),
            params![agent.owner, self.now - ANSWER_WINDOW_MS],
        )
        .map_err(db_err)?;
        Ok(RelayListResult {
            answers: self.full_all(&rows)?,
        })
    }

    // ---- Desk operations ----

    fn mark_shown(&self, rows: &mut [AskRow]) -> Result<(), String> {
        for row in rows.iter_mut().filter(|r| r.shown_ms.is_none()) {
            self.db
                .execute(
                    "UPDATE asks SET shown_ms = ?1 WHERE id = ?2",
                    params![self.now, row.id],
                )
                .map_err(db_err)?;
            self.event(
                "ask.shown",
                &row.id,
                json!({ "status": status_str(row.status) }),
            )?;
            row.shown_ms = Some(self.now);
        }
        Ok(())
    }

    fn list(&self) -> Result<ListResult, String> {
        let mut inbox = store::ask_rows(
            self.db,
            "status = 'open' ORDER BY muted, priority, deadline_ms, created_ms, rowid",
            [],
        )
        .map_err(db_err)?;
        self.mark_shown(&mut inbox)?;
        let digest_count = store::count(
            self.db,
            "SELECT COUNT(*) FROM asks WHERE status = 'digest'",
            [],
        )
        .map_err(db_err)?;
        let recent = store::ask_rows(
            self.db,
            &format!(
                "status IN ('answered', 'defaulted', 'cancelled') ORDER BY closed_ms DESC, rowid DESC LIMIT {RECENT_LIMIT}"
            ),
            [],
        )
        .map_err(db_err)?;
        Ok(ListResult {
            inbox: self.full_all(&inbox)?,
            digest_count,
            recent: self.full_all(&recent)?,
        })
    }

    fn open_digest(&self) -> Result<DigestResult, String> {
        let mut digest = store::ask_rows(
            self.db,
            "status = 'digest' ORDER BY deadline_ms, priority, created_ms, rowid",
            [],
        )
        .map_err(db_err)?;
        self.mark_shown(&mut digest)?;
        Ok(DigestResult {
            digest: self.full_all(&digest)?,
        })
    }

    fn desk_answer(
        &mut self,
        ask_id: &str,
        choice: Option<String>,
        values: Option<Value>,
        note: Option<String>,
        not_needed: bool,
    ) -> Result<AnswerResult, String> {
        let row = self
            .row(ask_id)?
            .ok_or_else(|| format!("no ask {ask_id}"))?;
        if !is_pending(row.status) {
            return Err(format!(
                "ask {ask_id} is already {}",
                status_str(row.status)
            ));
        }
        let (choice, values) = if not_needed {
            (None, None)
        } else {
            rules::check_answer(&row.input, choice, values)?
        };
        let note = note.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
        if note
            .as_ref()
            .is_some_and(|n| n.chars().count() > rules::MAX_NOTE)
        {
            return Err(format!(
                "a note takes at most {} characters",
                rules::MAX_NOTE
            ));
        }
        let via = if not_needed {
            AnswerVia::NotNeeded
        } else {
            AnswerVia::Desk
        };
        let answer = self.write_answer(&row, via, choice, values, note)?;
        Ok(AnswerResult { answer })
    }

    fn answer_token(&mut self, token: &str) -> Result<AnswerResult, String> {
        let parsed = tokens::parse(token)?;
        let toast: Option<(String, i64, Option<i64>, Option<i64>)> = self
            .db
            .query_row(
                "SELECT ask_id, expires_ms, used_ms, revoked_ms FROM toasts WHERE toast_id = ?1",
                [&parsed.toast_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .map_err(db_err)?;
        let Some((ask_id, expires_ms, used_ms, revoked_ms)) = toast else {
            return Err("unknown answer token".into());
        };
        if !tokens::verify(self.secret, &ask_id, &parsed) {
            return Err("invalid answer token".into());
        }
        if used_ms.is_some() {
            return Err("this answer token was already used".into());
        }
        let row = self
            .row(&ask_id)?
            .ok_or_else(|| format!("no ask {ask_id}"))?;
        if revoked_ms.is_some() || !is_pending(row.status) {
            return Err(format!(
                "ask {ask_id} is already {}; this token no longer works",
                status_str(row.status)
            ));
        }
        if self.now >= expires_ms {
            return Err("this answer token has expired; answer on the Desk".into());
        }
        if !rules::toast_buttons_allowed(&row.input) {
            return Err(format!("ask {ask_id} can't be answered from a toast"));
        }
        let choice = row
            .input
            .options
            .get(parsed.option_index)
            .cloned()
            .ok_or("invalid answer token")?;
        self.db
            .execute(
                "UPDATE toasts SET used_ms = ?1, used_option = ?2 WHERE toast_id = ?3",
                params![self.now, parsed.option_index as i64, parsed.toast_id],
            )
            .map_err(db_err)?;
        self.event(
            "token.used",
            &ask_id,
            json!({ "toast": parsed.toast_id, "option": parsed.option_index }),
        )?;
        let answer = self.write_answer(&row, AnswerVia::Toast, Some(choice), None, None)?;
        Ok(AnswerResult { answer })
    }

    fn set_owner(&self, owner: Owner) -> Result<Owner, String> {
        let name = owner.name.trim().to_string();
        if name.is_empty() || name.chars().count() > 300 || name.chars().any(char::is_control) {
            return Err("an owner name takes 1-300 characters without control characters".into());
        }
        if !matches!(owner.binding.as_str(), "config" | "approved" | "pending") {
            return Err("binding is config, approved or pending".into());
        }
        let o = Owner { name, ..owner };
        store::upsert_owner(self.db, &o, self.now).map_err(db_err)?;
        self.event(
            "owner.set",
            &o.name,
            json!({ "binding": o.binding, "codex_interactive": o.codex_interactive, "relay": o.relay, "muted":o.muted }),
        )?;
        store::owner(self.db, &o.name)
            .map_err(db_err)?
            .ok_or_else(|| "The saved owner disappeared".into())
    }

    fn set_settings(&mut self, s: Settings) -> Result<Settings, String> {
        let ntfy_topic_url = s
            .ntfy_topic_url
            .map(|u| u.trim().to_string())
            .filter(|u| !u.is_empty());
        // The scheme is stored in lower case (`HTTPS://` is accepted), which is what the daemon's push checks.
        let ntfy_topic_url = match ntfy_topic_url {
            Some(u)
                if u.len() <= 500 && !u.chars().any(|c| c.is_whitespace() || c.is_control()) =>
            {
                Some(
                    rules::https_lowercase_scheme(&u)
                        .ok_or("ntfy_topic_url must be an https:// URL without spaces")?,
                )
            }
            Some(_) => return Err("ntfy_topic_url must be an https:// URL without spaces".into()),
            None => None,
        };
        let s = Settings {
            ntfy_topic_url,
            ..s
        };
        store::set_settings(self.db, &s).map_err(db_err)?;
        self.event(
            "settings.set",
            "settings",
            json!({ "toasts_off": s.toasts_off, "ntfy": s.ntfy_topic_url.is_some() }),
        )?;
        Ok(s)
    }
}

impl Service for StoreService {
    fn mcp(&self, client: &ClientInfo, op: McpOp) -> Result<(Value, Vec<Effect>), String> {
        let now = self.now_ms();
        match op {
            McpOp::Register {
                owner,
                client: name,
                session_ref,
                cwd,
                doing,
                provisional_key,
            } => {
                let r = Registration {
                    owner,
                    client: name,
                    session_ref,
                    cwd,
                    doing,
                    provisional_key,
                };
                self.write_json(now, |c| c.register(client, &self.machine, r))
            }
            McpOp::Heartbeat { handle, doing } => self.write_json(now, |c| {
                c.touch(&handle, client)?;
                if let Some(d) = doing {
                    let d = rules::clean_line(&d, 200);
                    c.db.execute(
                        "UPDATE agents SET doing = ?1 WHERE handle = ?2",
                        params![(!d.is_empty()).then_some(d), handle],
                    )
                    .map_err(db_err)?;
                }
                store::agent(c.db, &handle)
                    .map_err(db_err)?
                    .ok_or_else(|| UNKNOWN_HANDLE.to_string())
            }),
            McpOp::Ask { handle, ask } => self.write_json(now, |c| c.ask(client, &handle, &ask)),
            // Returns at once; the daemon does any waiting by asking again (`wait_ms` is the daemon's to honour).
            McpOp::Answers {
                handle,
                ids,
                wait_ms: _,
            } => self.write_json(now, |c| c.answers(client, &handle, &ids)),
            McpOp::Cancel { handle, id } => {
                self.write_json(now, |c| c.cancel(client, &handle, &id))
            }
            McpOp::RelayList { handle } => self.write_json(now, |c| c.relay_list(client, &handle)),
            McpOp::RelayMark {
                handle,
                ask_id,
                to,
                ok,
                detail,
            } => self.write_json(now, |c| {
                c.relay_agent(client, &handle)?;
                let answer = c.deliver(&ask_id, &to, ok, detail.as_deref(), "relay")?;
                Ok(AnswerResult { answer })
            }),
            McpOp::Status => self.status(now),
        }
    }

    fn desk(&self, op: DeskOp) -> Result<(Value, Vec<Effect>), String> {
        let now = self.now_ms();
        match op {
            DeskOp::List => self.write_json(now, |c| c.list()),
            DeskOp::OpenDigest => self.write_json(now, |c| c.open_digest()),
            DeskOp::Answer {
                ask_id,
                choice,
                values,
                note,
            } => self.write_json(now, |c| c.desk_answer(&ask_id, choice, values, note, false)),
            DeskOp::NotNeeded { ask_id, note } => {
                self.write_json(now, |c| c.desk_answer(&ask_id, None, None, note, true))
            }
            DeskOp::SetMuted { owner, muted } => self.write_json(now, |c| {
                let mut o = store::owner(c.db, &owner)
                    .map_err(db_err)?
                    .ok_or("Unknown owner")?;
                o.muted = muted;
                c.set_owner(o)
            }),
            DeskOp::AnswerToken { token } => self.write_json(now, |c| c.answer_token(&token)),
            DeskOp::Agents => self.write_json(now, |c| {
                Ok(AgentsResult {
                    agents: store::agents(c.db).map_err(db_err)?,
                })
            }),
            DeskOp::Owners => self.write_json(now, |c| {
                let (owners, more_provisional) =
                    store::owners_for_desk(c.db, rules::DESK_PROVISIONAL_OWNERS).map_err(db_err)?;
                Ok(OwnersResult {
                    owners,
                    more_provisional,
                })
            }),
            DeskOp::SetOwner { owner } => self.write_json(now, |c| c.set_owner(owner)),
            DeskOp::GetSettings => self.write_json(now, |c| c.settings()),
            DeskOp::SetSettings { settings } => self.write_json(now, |c| c.set_settings(settings)),
            DeskOp::Status => self.status(now),
        }
    }

    fn tick(&self, now: OffsetDateTime) -> Vec<Effect> {
        let now = store::to_ms(now);
        self.write(now, |c| {
            c.apply_defaults()?;
            c.due_toasts()?;
            let expired =
                c.db.execute(
                    "DELETE FROM agents WHERE last_seen_ms < ?1",
                    [now - HANDLE_RETENTION_MS],
                )
                .map_err(db_err)?;
            if expired > 0 {
                c.event("agents.expired", "agents", json!({ "count": expired }))?;
            }
            let pruned =
                store::prune_provisional_owners(c.db, now - rules::PROVISIONAL_OWNER_RETENTION_MS)
                    .map_err(db_err)?;
            if pruned > 0 {
                c.event("owners.pruned", "owners", json!({ "count": pruned }))?;
            }
            Ok(())
        })
        .map(|(_, effects)| effects)
        .unwrap_or_default()
    }

    fn disconnected(&self, connection: u64) {
        let conn = self.lock();
        let _ = conn.execute(
            "UPDATE agents SET connection = NULL WHERE connection = ?1",
            [connection as i64],
        );
    }
}
