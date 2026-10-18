//! The count-only phone push (PLAN.md, "Phone (v1, count only)"): one POST to the ntfy topic URL Artem set on the
//! Desk, such as "2 decisions waiting, next due in 3 h". No ask content, at most one push a minute, and only when
//! the counts changed since the last push.

use std::time::{Duration, Instant};

pub const MIN_GAP: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Counts {
    pub open: u32,
    pub digest: u32,
    /// RFC 3339; the earliest open deadline.
    pub next_deadline: Option<String>,
}

#[derive(Debug, Default)]
pub struct PushState {
    last_sent: Option<(u32, u32)>,
    last_at: Option<Instant>,
    pending: Option<Counts>,
}

impl PushState {
    pub fn on_counts(&mut self, counts: Counts) {
        self.pending = Some(counts);
    }

    /// The counts to push now, if any: changed since the last push, not all zero, and a minute since the last
    /// push. A change that arrives inside the minute waits for it (the latest counts win).
    pub fn due(&mut self, now: Instant) -> Option<Counts> {
        let counts = self.pending.as_ref()?;
        let key = (counts.open, counts.digest);
        if self.last_sent == Some(key) {
            self.pending = None;
            return None;
        }
        if key == (0, 0) {
            // Everything was answered: nothing to announce, but the next ask counts as a change.
            self.last_sent = Some(key);
            self.pending = None;
            return None;
        }
        if self.last_at.is_some_and(|at| now.duration_since(at) < MIN_GAP) {
            return None;
        }
        self.last_sent = Some(key);
        self.last_at = Some(now);
        self.pending.take()
    }
}

/// "2 decisions waiting, 3 more in the digest, next due in 3 h".
pub fn body(counts: &Counts, now: time::OffsetDateTime) -> String {
    let plural = |n: u32, one: &str, many: &str| if n == 1 { format!("{n} {one}") } else { format!("{n} {many}") };
    let mut out = plural(counts.open, "decision waiting", "decisions waiting");
    if counts.digest > 0 {
        out.push_str(&format!(", {} in the digest", counts.digest));
    }
    if counts.open > 0
        && let Some(deadline) = counts.next_deadline.as_deref().and_then(|d| {
            time::OffsetDateTime::parse(d, &time::format_description::well_known::Rfc3339).ok()
        })
    {
        let minutes = (deadline - now).whole_minutes();
        let when = if minutes <= 0 {
            "next one is past its deadline".to_string()
        } else if minutes < 60 {
            "next due in under 1 h".to_string()
        } else if minutes < 48 * 60 {
            format!("next due in {} h", minutes / 60)
        } else {
            format!("next due in {} days", minutes / (24 * 60))
        };
        out.push_str(", ");
        out.push_str(&when);
    }
    out
}

/// POST the body with a `Title: Agentbolt` header. Only `https://` topic URLs are used.
pub fn post(topic_url: &str, body: &str) -> Result<(), String> {
    if !topic_url.starts_with("https://") {
        return Err("the ntfy topic URL must start with https://".into());
    }
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(15))).build().into();
    agent
        .post(topic_url)
        .header("Title", "Agentbolt")
        .send(body)
        .map(|_| ())
        .map_err(|e| format!("ntfy push failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    fn counts(open: u32, digest: u32) -> Counts {
        Counts { open, digest, next_deadline: None }
    }

    #[test]
    fn pushes_only_on_change_and_at_most_once_a_minute() {
        let t0 = Instant::now();
        let mut s = PushState::default();
        assert_eq!(s.due(t0), None, "nothing pending");
        s.on_counts(counts(2, 0));
        assert_eq!(s.due(t0), Some(counts(2, 0)));
        s.on_counts(counts(2, 0));
        assert_eq!(s.due(t0 + Duration::from_secs(120)), None, "same counts: no push");
        s.on_counts(counts(3, 0));
        assert_eq!(s.due(t0 + Duration::from_secs(10)), None, "inside the minute: held");
        s.on_counts(counts(4, 1));
        assert_eq!(s.due(t0 + Duration::from_secs(59)), None);
        assert_eq!(s.due(t0 + Duration::from_secs(61)), Some(counts(4, 1)), "the latest counts after the minute");
        s.on_counts(counts(0, 0));
        assert_eq!(s.due(t0 + Duration::from_secs(200)), None, "all answered: no push");
        s.on_counts(counts(1, 0));
        assert_eq!(s.due(t0 + Duration::from_secs(201)), Some(counts(1, 0)), "a new ask after zero is a change");
    }

    #[test]
    fn the_body_has_counts_only() {
        let now = datetime!(2026-10-18 12:00 UTC);
        let mut c = Counts { open: 2, digest: 0, next_deadline: Some("2026-10-18T15:10:00Z".into()) };
        assert_eq!(body(&c, now), "2 decisions waiting, next due in 3 h");
        c.open = 1;
        c.digest = 3;
        c.next_deadline = Some("2026-10-18T12:30:00Z".into());
        assert_eq!(body(&c, now), "1 decision waiting, 3 in the digest, next due in under 1 h");
        c.next_deadline = Some("2026-10-21T12:30:00Z".into());
        assert_eq!(body(&c, now), "1 decision waiting, 3 in the digest, next due in 3 days");
        c.next_deadline = Some("2026-10-18T11:00:00Z".into());
        assert_eq!(body(&c, now), "1 decision waiting, 3 in the digest, next one is past its deadline");
        c.next_deadline = Some("garbage".into());
        assert_eq!(body(&c, now), "1 decision waiting, 3 in the digest");
    }

    #[test]
    fn only_https_topics_are_posted() {
        assert!(post("http://ntfy.sh/x", "1 decision waiting").is_err());
        assert!(post("file:///c:/x", "1 decision waiting").is_err());
    }
}
