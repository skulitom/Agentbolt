//! Desk adoption measurements from recorded data. Baselines are immutable snapshots, never invented.
use crate::{
    events,
    store::{self, db_err, fmt_ms},
};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::json;

const DAY: i64 = 86_400_000;
const WINDOW: i64 = 14 * DAY;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OwnerStats {
    pub owner: String,
    pub submitted: u32,
    pub answered: u32,
    pub defaulted: u32,
    pub cancelled: u32,
    pub not_needed: u32,
    pub unanswered_deadlines: u32,
    pub not_needed_share: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Period {
    #[serde(default)]
    pub history_span_days: Option<f64>,
    pub from: String,
    pub to: String,
    pub submitted: u32,
    pub human_answers: u32,
    pub defaults: u32,
    pub cancelled: u32,
    pub not_needed: u32,
    pub median_answer_hours: Option<f64>,
    pub unanswered_deadlines: u32,
    pub deadlines_elapsed: u32,
    pub unanswered_deadline_share: Option<f64>,
    pub submissions_by_day: Vec<(String, u32)>,
    pub owners: Vec<OwnerStats>,
    #[serde(default)]
    pub collision_checks: u32,
    #[serde(default)]
    pub overlapping_checks: u32,
    #[serde(default)]
    pub unknown_checks: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Baseline {
    pub label: String,
    pub recorded_at: String,
    pub period: Period,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Verdict {
    pub recorded_at: String,
    pub text: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Report {
    pub current: Period,
    pub previous: Period,
    pub baseline: Option<Baseline>,
    pub verdict: Option<Verdict>,
    pub reversible_waiting_over_48h: u32,
    pub irreversible_open: u32,
    pub oldest_irreversible_hours: Option<f64>,
    pub observation_days: Option<f64>,
    #[serde(default)]
    pub observation: Option<Period>,
    pub explanation: String,
}

fn period(db: &Connection, from: i64, to: i64) -> Result<Period, String> {
    // Cohorts are asks submitted in this interval, with outcomes observed by its end. Half-open periods
    // avoid counting a boundary twice; late answers cannot retroactively make a past period look better.
    let mut stmt=db.prepare("SELECT a.owner,a.created_ms,a.deadline_ms,a.closed_ms,a.status,b.via,b.answered_ms FROM asks a LEFT JOIN answers b ON b.ask_id=a.id WHERE a.created_ms>=?1 AND a.created_ms<?2 ORDER BY a.created_ms").map_err(db_err)?;
    let rows = stmt
        .query_map(params![from, to], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, Option<i64>>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<i64>>(6)?,
            ))
        })
        .map_err(db_err)?;
    let first_record: Option<i64> = db
        .query_row(
            "SELECT CAST(MIN(unixepoch(at,'subsec'))*1000 AS INTEGER) FROM events",
            [],
            |r| r.get(0),
        )
        .map_err(db_err)?;
    let mut report = Period {
        history_span_days: first_record
            .filter(|first| *first < to)
            .map(|first| (to - from.max(first)).max(0) as f64 / DAY as f64),
        from: fmt_ms(from),
        to: fmt_ms(to),
        ..Default::default()
    };
    let mut owners = std::collections::BTreeMap::<String, OwnerStats>::new();
    let mut days = std::collections::BTreeMap::<String, u32>::new();
    let mut latency = Vec::new();
    for row in rows {
        let (owner, created, deadline, closed, status, via, answered) = row.map_err(db_err)?;
        report.submitted += 1;
        *days
            .entry(store::from_ms(created).date().to_string())
            .or_default() += 1;
        let o = owners.entry(owner.clone()).or_insert_with(|| OwnerStats {
            owner,
            ..Default::default()
        });
        o.submitted += 1;
        let human =
            answered.is_some_and(|at| at < to) && via.as_deref().is_some_and(|v| v != "defaulted");
        let defaulted = answered.is_some_and(|at| at < to) && via.as_deref() == Some("defaulted");
        let cancelled = status == "cancelled" && closed.is_some_and(|at| at < to);
        if human {
            report.human_answers += 1;
            o.answered += 1;
            latency.push(answered.unwrap_or(created).saturating_sub(created).max(0));
            if via.as_deref() == Some("not_needed") {
                report.not_needed += 1;
                o.not_needed += 1;
            }
        }
        if defaulted {
            report.defaults += 1;
            o.defaulted += 1;
        }
        if cancelled {
            report.cancelled += 1;
            o.cancelled += 1;
        }
        // Withdrawn asks before the deadline no longer required a decision.
        if deadline < to && !(cancelled && closed.is_some_and(|at| at <= deadline)) {
            report.deadlines_elapsed += 1;
            if answered.is_none_or(|at| at > deadline) {
                report.unanswered_deadlines += 1;
                o.unanswered_deadlines += 1;
            }
        }
    }
    latency.sort_unstable();
    if !latency.is_empty() {
        let mid = latency.len() / 2;
        let median = if latency.len() % 2 == 0 {
            (latency[mid - 1] as f64 + latency[mid] as f64) / 2.0
        } else {
            latency[mid] as f64
        };
        report.median_answer_hours = Some(median / 3_600_000.0);
    }
    report.unanswered_deadline_share = (report.deadlines_elapsed > 0)
        .then(|| f64::from(report.unanswered_deadlines) / f64::from(report.deadlines_elapsed));
    report.owners = owners
        .into_values()
        .map(|mut o| {
            o.not_needed_share =
                (o.submitted > 0).then(|| f64::from(o.not_needed) / f64::from(o.submitted));
            o
        })
        .collect();
    report.submissions_by_day = days.into_iter().collect();
    let (checks,overlap,unknown):(u32,u32,u32)=db.query_row("SELECT COUNT(*),COALESCE(SUM(json_extract(data,'$.collisions')>0),0),COALESCE(SUM(json_extract(data,'$.unknown')>0),0) FROM events WHERE kind='holds.checked' AND unixepoch(at,'subsec')>=?1 AND unixepoch(at,'subsec')<?2",params![from as f64/1000.0,to as f64/1000.0],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(db_err)?;
    report.collision_checks = checks;
    report.overlapping_checks = overlap;
    report.unknown_checks = unknown;
    Ok(report)
}

fn saved<T: serde::de::DeserializeOwned>(db: &Connection, key: &str) -> Result<Option<T>, String> {
    store::meta(db, key)
        .map_err(db_err)?
        .map(|raw| serde_json::from_str(&raw).map_err(|e| format!("saved {key}: {e}")))
        .transpose()
}

pub fn report(db: &Connection, now: i64) -> Result<Report, String> {
    let baseline: Option<Baseline> = saved(db, "adoption_baseline")?;
    let (stale,irreversible,oldest):(u32,u32,Option<i64>)=db.query_row("SELECT COALESCE(SUM(reversible=1 AND created_ms<=?1),0),COALESCE(SUM(reversible=0),0),MIN(CASE WHEN reversible=0 THEN created_ms END) FROM asks WHERE status IN ('open','digest')",[now-2*DAY],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(db_err)?;
    let observation_start = baseline
        .as_ref()
        .map(|b| {
            time::OffsetDateTime::parse(
                &b.recorded_at,
                &time::format_description::well_known::Rfc3339,
            )
            .map(store::to_ms)
            .map_err(|e| format!("Invalid saved baseline time: {e}"))
        })
        .transpose()?;
    let observation_days =
        observation_start.map(|start| (now - start).clamp(0, WINDOW) as f64 / DAY as f64);
    let observation = observation_start
        .map(|start| period(db, start, now.clamp(start, start.saturating_add(WINDOW))))
        .transpose()?;
    Ok(Report{
        current:period(db,now-WINDOW,now)?, previous:period(db,now-2*WINDOW,now-WINDOW)?,
        baseline, verdict:saved(db,"adoption_verdict")?, reversible_waiting_over_48h:stale,
        irreversible_open:irreversible,oldest_irreversible_hours:oldest.map(|at|(now-at).max(0) as f64/3_600_000.0),observation_days,observation,
        explanation:"Each period follows questions submitted during those 14 days, using outcomes recorded by the period's end. Answer latency measures recorded Desk/notification/phone answers, including Not needed; defaults are separate. Opening a session does not establish that its required go-ahead was given; that needs the session record. Not needed share uses all submitted asks for each owner. Deadline misses count asks still unanswered at their deadline. Missing history is not a zero-activity baseline. History span starts at the first stored event and does not prove uptime. The adoption comparison uses the fixed first 14 days after the saved baseline; rolling periods remain available separately. Stale and irreversible backlog are current totals across all ages.".into(),
    })
}

pub fn baseline(db: &Connection, now: i64, label: &str) -> Result<Baseline, String> {
    let label = label.trim();
    if label.is_empty() || label.chars().count() > 200 || label.chars().any(char::is_control) {
        return Err("baseline label needs 1–200 characters".into());
    }
    if saved::<Baseline>(db, "adoption_baseline")?.is_some() {
        return Err(
            "A baseline is already recorded; it stays fixed for the comparison period".into(),
        );
    }
    let result = Baseline {
        label: label.into(),
        recorded_at: fmt_ms(now),
        period: period(db, now - WINDOW, now)?,
    };
    store::set_meta(db, "adoption_baseline", &json!(result).to_string()).map_err(db_err)?;
    events::append(db, now, "adoption.baseline", "adoption", &json!(result)).map_err(db_err)?;
    Ok(result)
}

pub fn verdict(db: &Connection, now: i64, text: &str) -> Result<Verdict, String> {
    let text = text.trim();
    if text.is_empty()
        || text.len() > 4000
        || text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\r' | '\n' | '\t'))
    {
        return Err("verdict needs 1–4000 bytes of plain text".into());
    }
    let result = Verdict {
        recorded_at: fmt_ms(now),
        text: text.into(),
    };
    store::set_meta(db, "adoption_verdict", &json!(result).to_string()).map_err(db_err)?;
    events::append(db, now, "adoption.verdict", "adoption", &json!(result)).map_err(db_err)?;
    Ok(result)
}
