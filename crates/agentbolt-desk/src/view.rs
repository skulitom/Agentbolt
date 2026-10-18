//! The Desk's view-model: ordering, relative deadlines, the link filter, answer options and form values.
//! No window here, so all of it is tested headless.

use std::collections::HashMap;
use std::path::PathBuf;

use agentbolt_core::model::{Ask, AskKind, Evidence, Field};
use serde_json::{Map, Value};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

pub fn parse_time(s: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(s.trim(), &Rfc3339).ok()
}

/// Inbox order: priority (P0 first), then the nearest deadline, then the oldest.
pub fn sort_asks(asks: &mut [Ask]) {
    asks.sort_by(|a, b| {
        let da = parse_time(&a.input.deadline);
        let db = parse_time(&b.input.deadline);
        a.input
            .priority
            .cmp(&b.input.priority)
            .then_with(|| match (da, db) {
                (Some(x), Some(y)) => x.cmp(&y),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            })
            .then_with(|| a.created_at.cmp(&b.created_at))
    });
}

fn span(secs: i64) -> String {
    let min = secs / 60;
    if min < 1 {
        "under a minute".into()
    } else if min < 60 {
        format!("{min} min")
    } else if min < 48 * 60 {
        let (h, m) = (min / 60, min % 60);
        if h < 10 && m != 0 { format!("{h} h {m} min") } else { format!("{h} h") }
    } else {
        format!("{} days", min / (24 * 60))
    }
}

/// "due in 3 h 20 min" or "2 h overdue"; the flag is true when overdue.
pub fn relative_deadline(deadline: &str, now: OffsetDateTime) -> (String, bool) {
    match parse_time(deadline) {
        None => (format!("due {deadline}"), false),
        Some(d) if d >= now => (format!("due in {}", span((d - now).whole_seconds())), false),
        Some(d) => (format!("{} overdue", span((now - d).whole_seconds())), true),
    }
}

/// "5 min ago", for last-seen columns.
pub fn ago(when: &str, now: OffsetDateTime) -> String {
    match parse_time(when) {
        Some(t) if t <= now => format!("{} ago", span((now - t).whole_seconds())),
        Some(_) => "just now".into(),
        None => when.to_string(),
    }
}

/// What the Desk may do with an evidence link. Ask bodies and links are untrusted: only `https:` and
/// `claude://` open (with the OS), local paths are shown in Explorer (never run), the rest is plain text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    Web(String),
    Session(String),
    Path(PathBuf),
    Text,
}

/// Printable ASCII that URLs use; anything else (spaces, quotes, angle brackets, non-ASCII) disqualifies.
fn url_chars_ok(s: &str) -> bool {
    s.len() <= 2048
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._~:/?#[]@!$&'()*+,;=%".contains(&b))
}

fn has_prefix(s: &str, prefix: &str) -> bool {
    s.len() > prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix)
}

pub fn classify(url: &str) -> Link {
    let u = url.trim();
    if has_prefix(u, "https://") && url_chars_ok(u) {
        return Link::Web(u.to_string());
    }
    if has_prefix(u, "claude://") && url_chars_ok(u) {
        return Link::Session(u.to_string());
    }
    // A local drive path such as C:\DEV\x or C:/DEV/x. Never UNC (\\server\share would reach the network).
    let b = u.as_bytes();
    let drive = b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/');
    if drive && !u.contains(['"', '<', '>', '|', '?', '*']) && !u.chars().any(char::is_control) && !u[2..].contains(':') {
        return Link::Path(PathBuf::from(u.replace('/', "\\")));
    }
    Link::Text
}

/// The first `claude://` link in the evidence: the Desk's "Open the session" button.
pub fn session_link(evidence: &[Evidence]) -> Option<String> {
    evidence.iter().find_map(|e| match classify(&e.url) {
        Link::Session(u) => Some(u),
        _ => None,
    })
}

/// The buttons for a choose / approve ask.
pub fn options_for(ask: &Ask) -> Vec<String> {
    match ask.input.kind {
        AskKind::Approve if ask.input.options.is_empty() => vec!["Approve".into(), "Decline".into()],
        AskKind::Choose | AskKind::Approve => ask.input.options.clone(),
        _ => Vec::new(),
    }
}

pub fn is_default(ask: &Ask, option: &str) -> bool {
    ask.input.default.as_str().is_some_and(|d| d.trim() == option.trim())
}

/// A button's label: the default is marked.
pub fn option_label(ask: &Ask, option: &str) -> String {
    if is_default(ask, option) { format!("{option} (recommended)") } else { option.to_string() }
}

/// "reversible" or "can't be undone".
pub fn reversible_tag(ask: &Ask) -> &'static str {
    if ask.input.reversible { "reversible" } else { "can't be undone" }
}

/// What the `--url` handler was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UrlAction {
    Answer { token: String },
    Open,
}

fn percent_decode(s: &str) -> Result<String, String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                let hex = s.get(i + 1..i + 3).ok_or("a broken %-escape")?;
                out.push(u8::from_str_radix(hex, 16).map_err(|_| "a broken %-escape")?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| "the token isn't UTF-8".into())
}

/// Parse `<protocol>://answer?t=<token>` or `<protocol>://open` (Windows may add a `/` after the host).
pub fn parse_url(url: &str, protocol: &str) -> Result<UrlAction, String> {
    let rest = url
        .split_once("://")
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case(protocol))
        .map(|(_, rest)| rest)
        .ok_or_else(|| format!("this handler takes {protocol}:// links only"))?;
    let (host, query) = rest.split_once('?').unwrap_or((rest, ""));
    match host.trim_end_matches('/').to_ascii_lowercase().as_str() {
        "open" => Ok(UrlAction::Open),
        "answer" => {
            let token = query
                .split('&')
                .find_map(|kv| kv.strip_prefix("t="))
                .ok_or("the answer link has no token")?;
            let token = percent_decode(token)?;
            if token.is_empty() || token.len() > 512 || token.chars().any(char::is_control) {
                return Err("the answer link's token is malformed".into());
            }
            Ok(UrlAction::Answer { token })
        }
        other => Err(format!("unknown {protocol}:// action '{other}'")),
    }
}

/// One form field's input on the Desk.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldInput {
    Text(String),
    Bool(bool),
}

/// Inputs prefilled from the ask's default (an object of field values).
pub fn prefill(fields: &[Field], default: &Value) -> HashMap<String, FieldInput> {
    fields
        .iter()
        .map(|f| {
            let d = default.get(&f.id);
            let input = match f.field_type.as_str() {
                "boolean" => FieldInput::Bool(d.and_then(Value::as_bool).unwrap_or(false)),
                _ => FieldInput::Text(match d {
                    Some(Value::String(s)) => s.clone(),
                    Some(Value::Number(n)) => n.to_string(),
                    _ => String::new(),
                }),
            };
            (f.id.clone(), input)
        })
        .collect()
}

/// The form's values to send, or what's missing.
pub fn form_values(fields: &[Field], inputs: &HashMap<String, FieldInput>) -> Result<Value, String> {
    let mut out = Map::new();
    for f in fields {
        let v = match (f.field_type.as_str(), inputs.get(&f.id)) {
            ("boolean", Some(FieldInput::Bool(b))) => Some(Value::Bool(*b)),
            ("boolean", _) => Some(Value::Bool(false)),
            ("number", Some(FieldInput::Text(t))) if !t.trim().is_empty() => {
                let t = t.trim();
                let n = match t.parse::<i64>() {
                    Ok(i) => Some(serde_json::Number::from(i)),
                    Err(_) => t.parse::<f64>().ok().and_then(serde_json::Number::from_f64),
                };
                Some(Value::Number(n.ok_or_else(|| format!("{} must be a number", f.label))?))
            }
            (_, Some(FieldInput::Text(t))) if !t.trim().is_empty() => Some(Value::String(t.trim().to_string())),
            _ => None,
        };
        match v {
            Some(v) => {
                out.insert(f.id.clone(), v);
            }
            None if f.required => return Err(format!("{} is required", f.label)),
            None => {}
        }
    }
    Ok(Value::Object(out))
}

/// The empty-state line for each list.
pub const EMPTY_INBOX: &str = "Nothing waiting. Agents ask you here.";
pub const EMPTY_DIGEST: &str = "The digest is empty. Asks over the daily budget wait here.";
pub const EMPTY_AGENTS: &str = "No agents have registered yet.";

#[cfg(test)]
mod tests {
    use super::*;
    use agentbolt_core::model::{AskInput, AskStatus, Priority};
    use serde_json::json;
    use time::macros::datetime;

    fn ask(id: &str, priority: Priority, deadline: &str, created: &str) -> Ask {
        Ask {
            id: id.into(),
            owner: "o".into(),
            asking_session: None,
            input: AskInput {
                kind: AskKind::Choose,
                title: id.into(),
                body: String::new(),
                evidence: vec![],
                options: vec!["Ship".into(), "Wait".into()],
                fields: vec![],
                default: json!("Wait"),
                deadline: deadline.into(),
                reversible: true,
                priority,
                p0_reason: None,
                dedupe_key: None,
            },
            status: AskStatus::Open,
            created_at: created.into(),
            shown_at: None,
            answer: None,
        }
    }

    #[test]
    fn asks_sort_by_priority_then_deadline() {
        let mut v = vec![
            ask("p2-soon", Priority::P2, "2026-10-18T12:00:00Z", "1"),
            ask("p1-late", Priority::P1, "2026-10-25T12:00:00Z", "1"),
            ask("p1-soon", Priority::P1, "2026-10-19T12:00:00+01:00", "1"),
            ask("p0", Priority::P0, "2026-10-30T12:00:00Z", "1"),
            ask("p1-bad-deadline", Priority::P1, "whenever", "1"),
            ask("p1-soon-older", Priority::P1, "2026-10-19T11:00:00Z", "0"),
        ];
        sort_asks(&mut v);
        let ids: Vec<&str> = v.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["p0", "p1-soon-older", "p1-soon", "p1-late", "p1-bad-deadline", "p2-soon"]);
    }

    #[test]
    fn deadlines_read_relative() {
        let now = datetime!(2026-10-18 12:00 UTC);
        assert_eq!(relative_deadline("2026-10-18T15:20:00Z", now), ("due in 3 h 20 min".into(), false));
        assert_eq!(relative_deadline("2026-10-18T12:45:00Z", now), ("due in 45 min".into(), false));
        assert_eq!(relative_deadline("2026-10-18T12:00:30Z", now), ("due in under a minute".into(), false));
        assert_eq!(relative_deadline("2026-10-21T12:00:00Z", now), ("due in 3 days".into(), false));
        assert_eq!(relative_deadline("2026-10-19T02:00:00Z", now), ("due in 14 h".into(), false));
        assert_eq!(relative_deadline("2026-10-18T10:00:00Z", now), ("2 h overdue".into(), true));
        assert_eq!(relative_deadline("2026-10-18T13:00:00+02:00", now), ("1 h overdue".into(), true));
        assert_eq!(relative_deadline("soon", now), ("due soon".into(), false));
        assert_eq!(ago("2026-10-18T11:55:00Z", now), "5 min ago");
    }

    #[test]
    fn only_https_claude_and_local_paths_get_actions() {
        assert_eq!(classify("https://github.com/x/y/pull/1"), Link::Web("https://github.com/x/y/pull/1".into()));
        assert_eq!(classify("HTTPS://example.com/a?b=c&d=%20"), Link::Web("HTTPS://example.com/a?b=c&d=%20".into()));
        assert_eq!(classify("claude://session/abc"), Link::Session("claude://session/abc".into()));
        assert_eq!(classify("C:\\DEV\\Agentbolt\\README.md"), Link::Path(PathBuf::from("C:\\DEV\\Agentbolt\\README.md")));
        assert_eq!(classify("C:/DEV/x.rs"), Link::Path(PathBuf::from("C:\\DEV\\x.rs")));
        for text in [
            "http://example.com",
            "javascript:alert(1)",
            "file:///C:/Windows/System32/calc.exe",
            "https://exa mple.com",
            "https://example.com/\"onmouseover",
            "https://exämple.com",
            "\\\\server\\share\\x",
            "ms-settings:privacy",
            "crates/agentbolt/src/main.rs",
            "C:\\x\\a\"b",
            "C:\\x:stream",
            "https://",
            "",
        ] {
            assert_eq!(classify(text), Link::Text, "{text:?} must be plain text");
        }
        let ev = vec![
            Evidence { label: "PR".into(), url: "https://github.com".into() },
            Evidence { label: "Session".into(), url: "claude://resume/abc".into() },
        ];
        assert_eq!(session_link(&ev).as_deref(), Some("claude://resume/abc"));
        assert_eq!(session_link(&ev[..1]), None);
    }

    #[test]
    fn options_mark_the_default() {
        let a = ask("a", Priority::P1, "x", "0");
        assert_eq!(options_for(&a), vec!["Ship", "Wait"]);
        assert_eq!(option_label(&a, "Wait"), "Wait (recommended)");
        assert_eq!(option_label(&a, "Ship"), "Ship");
        let mut ap = a.clone();
        ap.input.kind = AskKind::Approve;
        ap.input.options.clear();
        ap.input.default = json!("Approve");
        assert_eq!(options_for(&ap), vec!["Approve", "Decline"]);
        assert_eq!(option_label(&ap, "Approve"), "Approve (recommended)");
        let mut t = a.clone();
        t.input.kind = AskKind::Text;
        assert!(options_for(&t).is_empty());
        assert_eq!(reversible_tag(&a), "reversible");
        t.input.reversible = false;
        assert_eq!(reversible_tag(&t), "can't be undone");
    }

    #[test]
    fn protocol_urls() {
        assert_eq!(parse_url("agentbolt-t1://answer?t=abc%2Fdef", "agentbolt-t1"), Ok(UrlAction::Answer { token: "abc/def".into() }));
        assert_eq!(parse_url("agentbolt-t1://answer/?x=1&t=tok", "agentbolt-t1"), Ok(UrlAction::Answer { token: "tok".into() }));
        assert_eq!(parse_url("agentbolt-t1://open", "agentbolt-t1"), Ok(UrlAction::Open));
        assert_eq!(parse_url("agentbolt-t1://open/", "agentbolt-t1"), Ok(UrlAction::Open));
        assert!(parse_url("agentbolt://answer?t=abc", "agentbolt-t1").is_err(), "another channel's link is refused");
        assert!(parse_url("agentbolt-t1://answer", "agentbolt-t1").is_err());
        assert!(parse_url("agentbolt-t1://answer?t=", "agentbolt-t1").is_err());
        assert!(parse_url("agentbolt-t1://answer?t=%zz", "agentbolt-t1").is_err());
        assert!(parse_url("agentbolt-t1://delete?t=x", "agentbolt-t1").is_err());
    }

    #[test]
    fn form_values_follow_the_fields() {
        let fields: Vec<Field> = serde_json::from_value(json!([
            {"id": "n", "label": "Count", "type": "number", "required": true},
            {"id": "ok", "label": "OK", "type": "boolean"},
            {"id": "c", "label": "Colour", "type": "choice", "options": ["red", "blue"]},
            {"id": "t", "label": "Note", "type": "text"}
        ]))
        .unwrap();
        let mut inputs = prefill(&fields, &json!({"n": 3, "c": "blue"}));
        assert_eq!(inputs["n"], FieldInput::Text("3".into()));
        assert_eq!(inputs["ok"], FieldInput::Bool(false));
        assert_eq!(form_values(&fields, &inputs).unwrap(), json!({"n": 3, "ok": false, "c": "blue"}));
        inputs.insert("n".into(), FieldInput::Text("three".into()));
        assert!(form_values(&fields, &inputs).unwrap_err().contains("number"));
        inputs.insert("n".into(), FieldInput::Text(" ".into()));
        assert!(form_values(&fields, &inputs).unwrap_err().contains("required"));
    }
}
