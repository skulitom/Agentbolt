//! The append-only audit log (PLAN.md, "Event"): one row for every change, hash-chained.
//!
//! Each row stores `prev_hash` and `hash = sha256(prev_hash || canonical JSON of the event)`, where the event is
//! `{"at", "data", "kind", "seq", "subject"}` with object keys sorted at every level. SQLite triggers refuse
//! UPDATE and DELETE on the table. As PLAN.md's Security section says, this catches accidents and sloppy edits,
//! not a careful attacker running as the same user (who can rewrite the whole chain).

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::store::fmt_ms;

/// `prev_hash` of the first event.
pub const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// JSON with object keys sorted at every level and no whitespace. Independent of serde_json's
/// `preserve_order` feature, which another crate in the build could switch on.
pub fn canonical_json(v: &Value) -> String {
    let mut out = String::new();
    write_canonical(v, &mut out);
    out
}

fn write_canonical(v: &Value, out: &mut String) {
    match v {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String((*k).clone()).to_string());
                out.push(':');
                write_canonical(&map[*k], out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        scalar => out.push_str(&scalar.to_string()),
    }
}

/// The hash of one event given the previous event's hash.
pub fn event_hash(prev_hash: &str, seq: i64, at: &str, kind: &str, subject: &str, data: &Value) -> String {
    let event = json!({ "seq": seq, "at": at, "kind": kind, "subject": subject, "data": data });
    let mut h = Sha256::new();
    h.update(prev_hash.as_bytes());
    h.update(canonical_json(&event).as_bytes());
    hex::encode(h.finalize())
}

/// Append one event; returns its hash. Call inside the transaction that makes the change.
pub fn append(conn: &Connection, at_ms: i64, kind: &str, subject: &str, data: &Value) -> rusqlite::Result<String> {
    let last: Option<(i64, String)> = conn
        .query_row("SELECT seq, hash FROM events ORDER BY seq DESC LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    let (seq, prev) = match last {
        Some((s, h)) => (s + 1, h),
        None => (1, GENESIS.to_string()),
    };
    let at = fmt_ms(at_ms);
    let hash = event_hash(&prev, seq, &at, kind, subject, data);
    conn.execute(
        "INSERT INTO events (seq, at, kind, subject, data, prev_hash, hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![seq, at, kind, subject, canonical_json(data), prev, hash],
    )?;
    Ok(hash)
}

/// Walk the whole chain. Ok(number of events) when every row links to the previous one and hashes to its
/// stored hash; Err names the first row that doesn't.
pub fn verify_chain(conn: &Connection) -> Result<u64, String> {
    let mut stmt = conn
        .prepare("SELECT seq, at, kind, subject, data, prev_hash, hash FROM events ORDER BY seq")
        .map_err(|e| format!("can't read the event log: {e}"))?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
            ))
        })
        .map_err(|e| format!("can't read the event log: {e}"))?;
    let mut expected_seq = 1i64;
    let mut prev = GENESIS.to_string();
    for row in rows {
        let (seq, at, kind, subject, data, prev_hash, hash) =
            row.map_err(|e| format!("can't read the event log: {e}"))?;
        if seq != expected_seq {
            return Err(format!("event {expected_seq} is missing from the log (next is {seq})"));
        }
        if prev_hash != prev {
            return Err(format!("event {seq} doesn't link to event {}", seq - 1));
        }
        let data: Value =
            serde_json::from_str(&data).map_err(|e| format!("event {seq} has unreadable data: {e}"))?;
        if event_hash(&prev, seq, &at, &kind, &subject, &data) != hash {
            return Err(format!("event {seq} was changed after it was written"));
        }
        prev = hash;
        expected_seq += 1;
    }
    Ok((expected_seq - 1) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        crate::store::open_memory().unwrap()
    }

    #[test]
    fn canonical_json_sorts_keys_at_every_level() {
        let v = json!({"b": 1, "a": {"z": [3, {"y": true, "x": null}], "c": "q\""}});
        assert_eq!(canonical_json(&v), r#"{"a":{"c":"q\"","z":[3,{"x":null,"y":true}]},"b":1}"#);
    }

    #[test]
    fn an_untouched_chain_verifies() {
        let c = db();
        assert_eq!(verify_chain(&c), Ok(0));
        for i in 0..5 {
            append(&c, 1_000 * i, "test", "s", &json!({"i": i})).unwrap();
        }
        assert_eq!(verify_chain(&c), Ok(5));
        let first_prev: String = c.query_row("SELECT prev_hash FROM events WHERE seq = 1", [], |r| r.get(0)).unwrap();
        assert_eq!(first_prev, GENESIS);
    }

    #[test]
    fn updates_and_deletes_are_refused_by_triggers() {
        let c = db();
        append(&c, 0, "test", "s", &json!({})).unwrap();
        assert!(c.execute("UPDATE events SET kind = 'x'", []).is_err());
        assert!(c.execute("DELETE FROM events", []).is_err());
    }

    #[test]
    fn a_tampered_row_is_detected() {
        let c = db();
        for i in 0..3 {
            append(&c, i, "test", "s", &json!({"choice": "Approve", "i": i})).unwrap();
        }
        // A sloppy edit: someone drops the guard and changes a row in place.
        c.execute_batch("DROP TRIGGER events_no_update; DROP TRIGGER events_no_delete;").unwrap();
        c.execute("UPDATE events SET data = '{\"choice\":\"Decline\",\"i\":1}' WHERE seq = 2", []).unwrap();
        assert_eq!(verify_chain(&c), Err("event 2 was changed after it was written".into()));
    }

    #[test]
    fn a_deleted_row_or_rewritten_hash_is_detected() {
        let c = db();
        for i in 0..3 {
            append(&c, i, "test", "s", &json!({"i": i})).unwrap();
        }
        c.execute_batch("DROP TRIGGER events_no_update; DROP TRIGGER events_no_delete;").unwrap();
        c.execute("DELETE FROM events WHERE seq = 2", []).unwrap();
        assert!(verify_chain(&c).unwrap_err().contains("missing"));

        let c = db();
        for i in 0..3 {
            append(&c, i, "test", "s", &json!({"i": i})).unwrap();
        }
        c.execute_batch("DROP TRIGGER events_no_update;").unwrap();
        c.execute("UPDATE events SET hash = ?1 WHERE seq = 1", [GENESIS]).unwrap();
        assert!(verify_chain(&c).is_err());
    }

    #[test]
    fn equivalent_json_formatting_is_not_tampering() {
        let c = db();
        append(&c, 0, "test", "s", &json!({"a": 1, "b": 2})).unwrap();
        c.execute_batch("DROP TRIGGER events_no_update;").unwrap();
        c.execute("UPDATE events SET data = '{ \"b\": 2, \"a\": 1 }'", []).unwrap();
        assert_eq!(verify_chain(&c), Ok(1));
    }
}
