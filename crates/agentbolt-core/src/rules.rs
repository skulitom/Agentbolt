//! Pure rules (PLAN.md "Rules for asks and work", "What an answer means", "The Desk"): input validation, the
//! link allowlist, answer validation, defaults, dedupe and toast eligibility.
//! No database access here; `service_impl` applies these against the store.

use std::collections::HashSet;

use serde_json::{Map, Value};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::model::{Answer, AnswerVia, AskInput, AskKind, Field, Priority};

/// Open asks per owner (including legacy digest rows during migration).
pub const OWNER_CAP: usize = 3;
pub const ACKNOWLEDGED: &str = "Acknowledged";

pub const MAX_TITLE: usize = 200;
pub const MAX_BODY: usize = 12_000;
pub const MAX_EVIDENCE: usize = 8;
pub const MAX_LABEL: usize = 160;
pub const MAX_OPTION: usize = 120;
pub const MIN_OPTIONS: usize = 2;
pub const MAX_OPTIONS: usize = 5;
pub const MAX_FIELDS: usize = 8;
pub const MAX_FIELD_CHOICES: usize = 20;
pub const MAX_LINK: usize = 2048;
pub const MAX_KEY: usize = 200;
pub const MAX_REASON: usize = 500;
pub const MAX_NOTE: usize = 2000;
pub const MAX_TEXT_ANSWER: usize = 12_000;
pub const MAX_DEADLINE_DAYS: i64 = 30;
/// Toast buttons: up to 5 options (PLAN.md, "Toasts (v1)").
pub const MAX_TOAST_BUTTONS: usize = 5;
/// A P2 ask still gets a toast when it is due within this long.
pub const URGENT_MS: i64 = 2 * 60 * 60 * 1000;
/// At most one popup per owner during this interval; waiting-count updates never pop up.
pub const TOAST_INTERVAL_MS: i64 = 10 * 60 * 1000;
/// A shown, reversible ask takes its default no sooner than this long after Artem was first shown it, even when
/// it was already overdue: opening the Desk after a day away mustn't default everything within a second.
pub const DEFAULT_GRACE_MS: i64 = 60 * 60 * 1000;
/// The Desk lists every bound or approved owner, plus at most this many of the most recent provisional ones.
pub const DESK_PROVISIONAL_OWNERS: u32 = 50;
/// Provisional owners with no asks and no activity for this long are pruned from the store.
pub const PROVISIONAL_OWNER_RETENTION_MS: i64 = 7 * DAY_MS;

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

pub fn priority_str(p: Priority) -> &'static str {
    match p {
        Priority::P0 => "P0",
        Priority::P1 => "P1",
        Priority::P2 => "P2",
    }
}

fn chars(s: &str) -> usize {
    s.chars().count()
}

fn has_control(s: &str) -> bool {
    s.chars().any(char::is_control)
}

/// One line of display text: control characters become spaces, runs of spaces collapse, cut to `max` chars.
pub fn clean_line(s: &str, max: usize) -> String {
    let spaced: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let collapsed = spaced.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(max).collect()
}

// ---- links ----

/// Printable URL characters only. Shared with the Desk so a stored link cannot bypass its launch filter.
pub fn url_chars_ok(s: &str) -> bool {
    s.len() <= MAX_LINK
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._~:/?#[]@!$&'()*+,;=%".contains(&b))
}

/// The Desk's link allowlist: `https://`, `claude://` and `codex://`, or a plain repository path (no scheme, no `..`,
/// no control characters, not a network path). Everything else (`javascript:`, `file:`, `ms-msdt:`,
/// `search-ms:`, `http:`, `data:` ...) is refused, and the Desk renders refused text as plain text.
pub fn check_link(url: &str) -> Result<(), String> {
    if url.is_empty() {
        return Err("an evidence link is empty".into());
    }
    if url.len() > MAX_LINK {
        return Err(format!(
            "an evidence link is longer than {MAX_LINK} characters"
        ));
    }
    if has_control(url) {
        return Err("an evidence link contains control characters".into());
    }
    if url.trim() != url {
        return Err("an evidence link starts or ends with spaces".into());
    }
    let lower = url.to_ascii_lowercase();
    for scheme in ["https://", "claude://", "codex://"] {
        if let Some(rest) = lower.strip_prefix(scheme) {
            if rest.is_empty() || rest.starts_with('/') {
                return Err(format!("the link {url:?} has no host after {scheme}"));
            }
            if !url_chars_ok(url) {
                return Err(format!("the link {url:?} contains unsafe URL characters"));
            }
            return Ok(());
        }
    }
    // Not an allowed URL scheme, so it must be a plain path.
    if let Some(colon) = url.find(':') {
        let before = &url[..colon];
        let drive = before.len() == 1
            && before.bytes().all(|b| b.is_ascii_alphabetic())
            && matches!(url.as_bytes().get(colon + 1), Some(b'\\' | b'/'))
            && !url[colon + 1..].contains(':');
        if !drive {
            let looks_like_scheme = !before.is_empty()
                && before
                    .bytes()
                    .next()
                    .is_some_and(|b| b.is_ascii_alphabetic())
                && before
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"+-.".contains(&b));
            return Err(if looks_like_scheme {
                format!(
                    "the link {url:?} is refused: only https://, claude:// and codex:// links or repository paths are allowed, \
                     not {}:",
                    before.to_ascii_lowercase()
                )
            } else {
                format!("the path {url:?} is refused: ':' is allowed only after a drive letter")
            });
        }
    }
    if url.starts_with("\\\\")
        || url.starts_with("//")
        || url.starts_with("\\/")
        || url.starts_with("/\\")
    {
        return Err(format!(
            "the path {url:?} is refused: network paths aren't allowed"
        ));
    }
    if url.contains("..") {
        return Err(format!("the path {url:?} is refused: '..' isn't allowed"));
    }
    Ok(())
}

/// Does `s` start with `prefix`, ignoring ASCII case? Safe on any string: it never slices inside a character.
pub fn starts_with_ignore_ascii_case(s: &str, prefix: &str) -> bool {
    s.get(..prefix.len())
        .is_some_and(|p| p.eq_ignore_ascii_case(prefix))
}

/// An `https://` URL (any case) with its scheme in lower case and something after it; None otherwise.
pub fn https_lowercase_scheme(url: &str) -> Option<String> {
    const SCHEME: &str = "https://";
    if !starts_with_ignore_ascii_case(url, SCHEME) {
        return None;
    }
    let rest = url
        .get(SCHEME.len()..)
        .filter(|r| !r.is_empty() && !r.starts_with('/'))?;
    Some(format!("{SCHEME}{rest}"))
}

// ---- asks ----

/// Validate and normalize an ask. Returns the normalized input (trimmed text, approve options filled in, the
/// inform default set to "Acknowledged", empty keys dropped) and the deadline in Unix milliseconds.
pub fn validate_ask(input: &AskInput, now_ms: i64) -> Result<(AskInput, i64), String> {
    let mut a = input.clone();

    a.title = a.title.trim().to_string();
    if a.title.is_empty() || chars(&a.title) > MAX_TITLE {
        return Err(format!(
            "title takes 1-{MAX_TITLE} characters (got {})",
            chars(&a.title)
        ));
    }
    if has_control(&a.title) {
        return Err("title can't contain control characters or line breaks".into());
    }
    if chars(&a.body) > MAX_BODY {
        return Err(format!(
            "body takes at most {MAX_BODY} characters (got {})",
            chars(&a.body)
        ));
    }

    if a.evidence.len() > MAX_EVIDENCE {
        return Err(format!(
            "at most {MAX_EVIDENCE} evidence items (got {})",
            a.evidence.len()
        ));
    }
    for e in &mut a.evidence {
        e.label = e.label.trim().to_string();
        if chars(&e.label) > MAX_LABEL {
            return Err(format!(
                "evidence labels take at most {MAX_LABEL} characters"
            ));
        }
        if has_control(&e.label) {
            return Err("evidence labels can't contain control characters".into());
        }
        check_link(&e.url)?;
    }

    for o in &mut a.options {
        *o = o.trim().to_string();
    }
    match a.kind {
        AskKind::Choose => {
            check_options(&a.options)?;
            a.default = check_default_option(&a.default, &a.options)?;
        }
        AskKind::Approve => {
            if a.options.is_empty() {
                a.options = vec!["Approve".into(), "Decline".into()];
            }
            check_options(&a.options)?;
            a.default = check_default_option(&a.default, &a.options)?;
        }
        AskKind::Inform => {
            if !a.options.is_empty() {
                return Err("an inform ask takes no options".into());
            }
            match &a.default {
                Value::Null => {}
                Value::String(s) if s == ACKNOWLEDGED || s.is_empty() => {}
                _ => {
                    return Err(format!(
                        "an inform ask's default can only be \"{ACKNOWLEDGED}\""
                    ));
                }
            }
            a.default = Value::String(ACKNOWLEDGED.into());
        }
        AskKind::Text => {
            if !a.options.is_empty() {
                return Err("a text ask takes no options".into());
            }
            let Value::String(s) = &a.default else {
                return Err("a text ask's default must be a string".into());
            };
            if chars(s) > MAX_TEXT_ANSWER {
                return Err(format!(
                    "a text default takes at most {MAX_TEXT_ANSWER} characters"
                ));
            }
        }
        AskKind::Form => {
            if !a.options.is_empty() {
                return Err("a form ask takes fields, not options".into());
            }
            check_fields(&mut a.fields)?;
            if !a.default.is_object() {
                return Err("a form ask's default must be an object of field values".into());
            }
            a.default = check_form_values(&a.fields, &a.default)
                .map_err(|e| format!("the default: {e}"))?;
        }
    }
    if a.kind != AskKind::Form && !a.fields.is_empty() {
        return Err("only form asks take fields".into());
    }

    let deadline = OffsetDateTime::parse(a.deadline.trim(), &Rfc3339).map_err(|_| {
        format!(
            "deadline must be RFC 3339, such as 2026-10-20T18:00:00Z (got {:?})",
            a.deadline
        )
    })?;
    let deadline_ms = crate::store::to_ms(deadline);
    if deadline_ms <= now_ms {
        return Err("deadline must be in the future".into());
    }
    if deadline_ms > now_ms + MAX_DEADLINE_DAYS * DAY_MS {
        return Err(format!(
            "deadline must be at most {MAX_DEADLINE_DAYS} days away"
        ));
    }
    a.deadline = a.deadline.trim().to_string();

    a.p0_reason = a
        .p0_reason
        .map(|r| r.trim().to_string())
        .filter(|r| !r.is_empty());
    if let Some(r) = &a.p0_reason
        && chars(r) > MAX_REASON
    {
        return Err(format!("p0_reason takes at most {MAX_REASON} characters"));
    }
    if a.priority == Priority::P0 && a.p0_reason.is_none() {
        return Err("a P0 ask needs p0_reason: why it is urgent".into());
    }
    a.dedupe_key = a
        .dedupe_key
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty());
    if let Some(k) = &a.dedupe_key
        && chars(k) > MAX_KEY
    {
        return Err(format!("dedupe_key takes at most {MAX_KEY} characters"));
    }
    Ok((a, deadline_ms))
}

fn check_options(options: &[String]) -> Result<(), String> {
    if !(MIN_OPTIONS..=MAX_OPTIONS).contains(&options.len()) {
        return Err(format!(
            "takes {MIN_OPTIONS}-{MAX_OPTIONS} options (got {})",
            options.len()
        ));
    }
    let mut seen = HashSet::new();
    for o in options {
        if o.is_empty() || chars(o) > MAX_OPTION || has_control(o) {
            return Err(format!(
                "options take 1-{MAX_OPTION} characters on one line"
            ));
        }
        if !seen.insert(o.as_str()) {
            return Err(format!("the option {o:?} appears twice"));
        }
    }
    Ok(())
}

fn check_default_option(default: &Value, options: &[String]) -> Result<Value, String> {
    match default.as_str().map(str::trim) {
        Some(d) if options.iter().any(|o| o == d) => Ok(Value::String(d.to_string())),
        _ => Err(format!(
            "the default must be one of the options ({})",
            options.join(", ")
        )),
    }
}

fn is_snake_case(id: &str) -> bool {
    let b = id.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_')
        && !id.ends_with('_')
        && !id.contains("__")
}

fn check_fields(fields: &mut [Field]) -> Result<(), String> {
    if !(1..=MAX_FIELDS).contains(&fields.len()) {
        return Err(format!(
            "a form takes 1-{MAX_FIELDS} fields (got {})",
            fields.len()
        ));
    }
    let mut ids = HashSet::new();
    for f in fields.iter_mut() {
        if !is_snake_case(&f.id) {
            return Err(format!(
                "field id {:?} must be snake_case, such as due_date",
                f.id
            ));
        }
        if !ids.insert(f.id.clone()) {
            return Err(format!("field id {:?} appears twice", f.id));
        }
        f.label = f.label.trim().to_string();
        if f.label.is_empty() || chars(&f.label) > MAX_LABEL || has_control(&f.label) {
            return Err(format!(
                "field {:?} needs a label of 1-{MAX_LABEL} characters",
                f.id
            ));
        }
        match f.field_type.as_str() {
            "text" | "number" | "boolean" => {
                if f.options.as_ref().is_some_and(|o| !o.is_empty()) {
                    return Err(format!("field {:?}: only choice fields take options", f.id));
                }
                f.options = None;
            }
            "choice" => {
                let opts = f.options.get_or_insert_with(Vec::new);
                for o in opts.iter_mut() {
                    *o = o.trim().to_string();
                }
                if opts.len() < 2 || opts.len() > MAX_FIELD_CHOICES {
                    return Err(format!(
                        "choice field {:?} takes 2-{MAX_FIELD_CHOICES} options",
                        f.id
                    ));
                }
                let mut seen = HashSet::new();
                for o in opts.iter() {
                    if o.is_empty()
                        || chars(o) > MAX_OPTION
                        || has_control(o)
                        || !seen.insert(o.as_str())
                    {
                        return Err(format!(
                            "choice field {:?}: options must be distinct, 1-{MAX_OPTION} characters",
                            f.id
                        ));
                    }
                }
            }
            other => {
                return Err(format!(
                    "field {:?} has type {other:?}; types are text, number, boolean and choice",
                    f.id
                ));
            }
        }
    }
    Ok(())
}

/// Check form values against the fields: an object keyed by field id, no unknown keys, required fields present,
/// each value of its field's type. Returns the values with nulls dropped.
pub fn check_form_values(fields: &[Field], values: &Value) -> Result<Value, String> {
    let obj = values
        .as_object()
        .ok_or("form values must be a JSON object keyed by field id")?;
    if let Some(k) = obj.keys().find(|k| !fields.iter().any(|f| &f.id == *k)) {
        return Err(format!("{k:?} is not a field of this form"));
    }
    let mut out = Map::new();
    for f in fields {
        match obj.get(&f.id) {
            None | Some(Value::Null) => {
                if f.required {
                    return Err(format!("field {:?} is required", f.id));
                }
            }
            Some(v) => {
                let ok = match f.field_type.as_str() {
                    "text" => v.as_str().is_some_and(|s| chars(s) <= MAX_TEXT_ANSWER),
                    "number" => v.is_number(),
                    "boolean" => v.is_boolean(),
                    "choice" => v.as_str().is_some_and(|s| {
                        f.options.as_ref().is_some_and(|o| o.iter().any(|x| x == s))
                    }),
                    _ => false,
                };
                if !ok {
                    return Err(match f.field_type.as_str() {
                        "choice" => format!(
                            "field {:?} must be one of: {}",
                            f.id,
                            f.options.as_deref().unwrap_or_default().join(", ")
                        ),
                        t => format!("field {:?} must be a {t}", f.id),
                    });
                }
                out.insert(f.id.clone(), v.clone());
            }
        }
    }
    Ok(Value::Object(out))
}

/// Validate a Desk answer against its ask. Returns (choice, values) to store: choose/approve/inform/text answers
/// use `choice` (a text reply goes there; `values` as a JSON string is accepted too), form answers use `values`.
pub fn check_answer(
    input: &AskInput,
    choice: Option<String>,
    values: Option<Value>,
) -> Result<(Option<String>, Option<Value>), String> {
    let values = values.filter(|v| !v.is_null());
    match input.kind {
        AskKind::Choose | AskKind::Approve => {
            let c = choice.map(|c| c.trim().to_string()).ok_or_else(|| {
                format!(
                    "this ask needs a choice: one of {}",
                    input.options.join(", ")
                )
            })?;
            if !input.options.contains(&c) {
                return Err(format!(
                    "{c:?} is not one of the options ({})",
                    input.options.join(", ")
                ));
            }
            if values.is_some() {
                return Err("this ask takes a choice, not values".into());
            }
            Ok((Some(c), None))
        }
        AskKind::Inform => match choice.as_deref().map(str::trim) {
            None | Some("") => Ok((Some(ACKNOWLEDGED.into()), None)),
            Some(c) if c == ACKNOWLEDGED => Ok((Some(ACKNOWLEDGED.into()), None)),
            Some(c) => Err(format!(
                "an inform ask can only be acknowledged, not answered {c:?}"
            )),
        },
        AskKind::Text => {
            let text = match (choice, values) {
                (Some(c), _) => c,
                (None, Some(Value::String(s))) => s,
                _ => return Err("this ask needs a text reply".into()),
            };
            if text.trim().is_empty() {
                return Err("the reply is empty".into());
            }
            if chars(&text) > MAX_TEXT_ANSWER {
                return Err(format!(
                    "the reply takes at most {MAX_TEXT_ANSWER} characters"
                ));
            }
            Ok((Some(text), None))
        }
        AskKind::Form => {
            let v = values.ok_or("this ask needs form values")?;
            Ok((None, Some(check_form_values(&input.fields, &v)?)))
        }
    }
}

/// What a default applies: (choice, values), the same shapes `check_answer` returns.
pub fn default_answer(input: &AskInput) -> (Option<String>, Option<Value>) {
    match input.kind {
        AskKind::Form => (None, Some(input.default.clone())),
        AskKind::Inform => (Some(ACKNOWLEDGED.into()), None),
        _ => (input.default.as_str().map(|s| s.trim().to_string()), None),
    }
}

// ---- dedupe, budget, day ----

/// Lowercase, letters and digits only, single spaces: "Merge PR #12?" and "merge pr 12" match.
pub fn normalize_title(t: &str) -> String {
    let mapped: String = t
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    mapped
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Same dedupe key, or (unless both carry different keys) the same normalized title.
pub fn is_duplicate(existing: &AskInput, new: &AskInput) -> bool {
    match (&existing.dedupe_key, &new.dedupe_key) {
        (Some(a), Some(b)) => a == b,
        _ => normalize_title(&existing.title) == normalize_title(&new.title),
    }
}

/// When a shown, reversible ask may take its default: its deadline, but never before `DEFAULT_GRACE_MS` after it
/// was first shown.
pub fn default_due_ms(deadline_ms: i64, shown_ms: i64) -> i64 {
    deadline_ms.max(shown_ms.saturating_add(DEFAULT_GRACE_MS))
}

// ---- toasts and delivery text ----

/// Toast buttons are only for reversible choose/approve asks with up to 5 options.
pub fn toast_buttons_allowed(input: &AskInput) -> bool {
    input.reversible
        && matches!(input.kind, AskKind::Choose | AskKind::Approve)
        && !input.options.is_empty()
        && input.options.len() <= MAX_TOAST_BUTTONS
}

/// A new Inbox ask gets a toast when it is P0/P1 or due within 2 hours.
pub fn toast_worthy(input: &AskInput, deadline_ms: i64, now_ms: i64) -> bool {
    input.priority != Priority::P2 || deadline_ms - now_ms <= URGENT_MS
}

pub fn toast_body(owner: &str, body: &str) -> String {
    let excerpt = clean_line(body, 200);
    if excerpt.is_empty() {
        format!("From {owner}.")
    } else {
        format!("From {owner}. {excerpt}")
    }
}

fn answer_text(answer: &Answer) -> String {
    match (&answer.choice, &answer.values) {
        (Some(c), _) => format!("{c:?}"),
        (None, Some(v)) => v.to_string(),
        (None, None) => "(none)".into(),
    }
}

/// The message `codex queue` puts into the asking thread.
pub fn codex_text(ask_id: &str, title: &str, answer: &Answer) -> String {
    let what = answer_text(answer);
    match answer.via {
        AnswerVia::NotNeeded => format!(
            "Agentbolt: Artem marked your ask {ask_id} ({title:?}) Not needed. You should not have asked this question. No option was approved; this grants no permission.{}",
            answer
                .note
                .as_deref()
                .map(|n| format!(" Note: {n}"))
                .unwrap_or_default()
        ),
        AnswerVia::Defaulted => format!(
            "Agentbolt: your ask {ask_id} ({title:?}) reached its deadline and took its default: {what}. A default \
             carries no authority from Artem; act under your own rules."
        ),
        AnswerVia::Desk | AnswerVia::Toast => {
            let note = answer
                .note
                .as_deref()
                .map(|n| format!(" Note: {n}"))
                .unwrap_or_default();
            format!(
                "Agentbolt: Artem answered your ask {ask_id} ({title:?}): {what}.{note} It covers this ask only; it \
                 isn't permission for gated actions such as publishing, merging or money."
            )
        }
    }
}

pub fn check_owner_name(name: &str) -> Result<(), String> {
    if name.is_empty() || chars(name) > 64 {
        return Err("owner names take 1-64 characters".into());
    }
    if name.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("owner names can't contain spaces or control characters".into());
    }
    if name.starts_with("session:") {
        return Err(
            "owner names starting with 'session:' are provisional and can't be configured".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Evidence;
    use serde_json::json;

    const NOW: i64 = 1_792_400_000_000; // 2026-10-18 (UTC)

    fn deadline_in(ms: i64) -> String {
        crate::store::fmt_ms(NOW + ms)
    }

    fn choose() -> AskInput {
        AskInput {
            kind: AskKind::Choose,
            title: "Which colour?".into(),
            body: String::new(),
            evidence: vec![],
            options: vec!["Red".into(), "Blue".into()],
            fields: vec![],
            default: json!("Red"),
            deadline: deadline_in(DAY_MS),
            reversible: true,
            priority: Priority::P2,
            p0_reason: None,
            dedupe_key: None,
        }
    }

    fn field(id: &str, t: &str, required: bool, options: Option<Vec<&str>>) -> Field {
        Field {
            id: id.into(),
            label: format!("Label {id}"),
            field_type: t.into(),
            required,
            options: options.map(|o| o.into_iter().map(String::from).collect()),
        }
    }

    fn err(a: &AskInput) -> String {
        validate_ask(a, NOW).unwrap_err()
    }

    #[test]
    fn allowed_links() {
        for ok in [
            "https://github.com/skulitom/Agentbolt/pull/1",
            "HTTPS://EXAMPLE.COM/x",
            "claude://resume?session=abc",
            "src/main.rs",
            "crates/agentbolt-core/src/rules.rs#L10",
            "C:\\DEV\\Compass\\NOTES.md",
            "C:/DEV/My Project/a.md",
            "README.md",
            "a.b/c.d",
        ] {
            assert_eq!(check_link(ok), Ok(()), "{ok:?} should pass");
        }
    }

    #[test]
    fn refused_links() {
        for bad in [
            "javascript:alert(1)",
            "JavaScript:alert(1)",
            "file:///C:/Windows/System32/calc.exe",
            "file:C:/x",
            "ms-msdt:/id PCWDiagnostic",
            "search-ms:query=x&crumb=location:\\\\evil\\share",
            "http://example.com",
            "data:text/html,<script>alert(1)</script>",
            "vbscript:msgbox",
            "mailto:a@b.c",
            "ms-settings:",
            "https://",
            "https:///etc/passwd",
            "https:/example.com",
            "https://exa mple.com",
            "claude://",
            " javascript:alert(1)",
            "java\tscript:alert(1)",
            "\\\\server\\share\\x",
            "//server/share",
            "../secret",
            "docs/../../x",
            "C:\\x\\..\\y",
            "C:x",
            "C:\\x:stream",
            "a:b",
            "",
            "x\u{0}y",
        ] {
            assert!(check_link(bad).is_err(), "{bad:?} should be refused");
        }
        let e = check_link("ms-msdt:/id").unwrap_err();
        assert!(e.contains("ms-msdt:") && e.contains("https://"), "{e}");
    }

    #[test]
    fn a_good_ask_normalizes() {
        let mut a = choose();
        a.title = "  Which colour?  ".into();
        a.dedupe_key = Some("  ".into());
        let (n, dl) = validate_ask(&a, NOW).unwrap();
        assert_eq!(n.title, "Which colour?");
        assert_eq!(n.dedupe_key, None);
        assert_eq!(dl, NOW + DAY_MS);
    }

    #[test]
    fn title_body_and_evidence_limits() {
        let mut a = choose();
        a.title = " ".into();
        assert!(err(&a).contains("title"));
        a.title = "x".repeat(201);
        assert!(err(&a).contains("title"));
        a.title = "x".repeat(200);
        assert!(validate_ask(&a, NOW).is_ok());
        a.title = "two\nlines".into();
        assert!(err(&a).contains("control"));

        let mut a = choose();
        a.body = "b".repeat(12_001);
        assert!(err(&a).contains("body"));
        a.body = "b".repeat(12_000);
        assert!(validate_ask(&a, NOW).is_ok());

        let mut a = choose();
        let ev = Evidence {
            label: "PR".into(),
            url: "https://github.com/x".into(),
        };
        a.evidence = vec![ev.clone(); 9];
        assert!(err(&a).contains("evidence"));
        a.evidence = vec![ev.clone(); 8];
        assert!(validate_ask(&a, NOW).is_ok());
        a.evidence = vec![Evidence {
            label: "l".repeat(161),
            url: ev.url.clone(),
        }];
        assert!(err(&a).contains("label"));
        a.evidence = vec![Evidence {
            label: "bad".into(),
            url: "javascript:alert(1)".into(),
        }];
        assert!(err(&a).contains("javascript:"));
    }

    #[test]
    fn choose_and_approve_options() {
        let mut a = choose();
        a.options = vec!["One".into()];
        a.default = json!("One");
        assert!(err(&a).contains("2-5 options"));
        a.options = (0..6).map(|i| format!("o{i}")).collect();
        a.default = json!("o0");
        assert!(err(&a).contains("2-5 options"));
        a.options = vec!["A".into(), "A".into()];
        a.default = json!("A");
        assert!(err(&a).contains("twice"));
        a.options = vec!["A".into(), "B".into()];
        a.default = json!("C");
        assert!(err(&a).contains("default must be one of"));
        a.default = json!(1);
        assert!(err(&a).contains("default must be one of"));

        let mut a = choose();
        a.kind = AskKind::Approve;
        a.options = vec![];
        a.default = json!("Approve");
        let (n, _) = validate_ask(&a, NOW).unwrap();
        assert_eq!(n.options, vec!["Approve", "Decline"]);
        a.default = json!("Yes");
        assert!(err(&a).contains("default must be one of"));
    }

    #[test]
    fn inform_text_and_form_kinds() {
        let mut a = choose();
        a.kind = AskKind::Inform;
        assert!(err(&a).contains("no options"));
        a.options = vec![];
        a.default = Value::Null;
        assert_eq!(
            validate_ask(&a, NOW).unwrap().0.default,
            json!(ACKNOWLEDGED)
        );
        a.default = json!("Sure");
        assert!(err(&a).contains("Acknowledged"));

        let mut a = choose();
        a.kind = AskKind::Text;
        a.options = vec![];
        a.default = json!(5);
        assert!(err(&a).contains("string"));
        a.default = json!("");
        assert!(validate_ask(&a, NOW).is_ok());

        let mut a = choose();
        a.kind = AskKind::Form;
        a.options = vec![];
        a.default = json!({});
        assert!(err(&a).contains("1-8 fields"));
        a.fields = (0..9)
            .map(|i| field(&format!("f{i}"), "text", false, None))
            .collect();
        assert!(err(&a).contains("1-8 fields"));
        a.fields = vec![field("DueDate", "text", false, None)];
        assert!(err(&a).contains("snake_case"));
        a.fields = vec![
            field("a", "text", false, None),
            field("a", "number", false, None),
        ];
        assert!(err(&a).contains("twice"));
        a.fields = vec![field("a", "date", false, None)];
        assert!(err(&a).contains("types are"));
        a.fields = vec![field("a", "choice", false, Some(vec!["x"]))];
        assert!(err(&a).contains("2-20 options"));
        a.fields = vec![field("a", "text", false, Some(vec!["x", "y"]))];
        assert!(err(&a).contains("only choice fields"));
        a.fields = vec![
            field("name", "text", true, None),
            field("size", "choice", false, Some(vec!["S", "M"])),
        ];
        a.default = json!("x");
        assert!(err(&a).contains("object"));
        a.default = json!({"size": "S"});
        assert!(err(&a).contains("required"));
        a.default = json!({"name": "n", "size": "XL"});
        assert!(err(&a).contains("one of"));
        a.default = json!({"name": "n", "colour": "red"});
        assert!(err(&a).contains("not a field"));
        a.default = json!({"name": "n", "size": "M"});
        assert!(validate_ask(&a, NOW).is_ok());

        let mut a = choose();
        a.fields = vec![field("a", "text", false, None)];
        assert!(err(&a).contains("only form asks"));
    }

    #[test]
    fn deadlines_and_p0() {
        let mut a = choose();
        a.deadline = "tomorrow".into();
        assert!(err(&a).contains("RFC 3339"));
        a.deadline = deadline_in(-1000);
        assert!(err(&a).contains("future"));
        a.deadline = deadline_in(31 * DAY_MS);
        assert!(err(&a).contains("30 days"));
        a.deadline = deadline_in(30 * DAY_MS);
        assert!(validate_ask(&a, NOW).is_ok());
        a.deadline = "2026-10-20T18:00:00+01:00".into();
        assert!(validate_ask(&a, NOW).is_ok());

        let mut a = choose();
        a.priority = Priority::P0;
        assert!(err(&a).contains("p0_reason"));
        a.p0_reason = Some("   ".into());
        assert!(err(&a).contains("p0_reason"));
        a.p0_reason = Some("prod is down".into());
        assert!(validate_ask(&a, NOW).is_ok());
    }

    #[test]
    fn answers_are_checked_against_the_ask() {
        let a = validate_ask(&choose(), NOW).unwrap().0;
        assert_eq!(
            check_answer(&a, Some("Blue".into()), None),
            Ok((Some("Blue".into()), None))
        );
        assert!(check_answer(&a, Some("Green".into()), None).is_err());
        assert!(check_answer(&a, None, None).is_err());

        let mut t = choose();
        t.kind = AskKind::Text;
        t.options = vec![];
        t.default = json!("d");
        assert_eq!(
            check_answer(&t, None, Some(json!("hi"))),
            Ok((Some("hi".into()), None))
        );
        assert!(check_answer(&t, Some("  ".into()), None).is_err());
        assert!(check_answer(&t, None, None).is_err());

        let mut i = choose();
        i.kind = AskKind::Inform;
        assert_eq!(
            check_answer(&i, None, None),
            Ok((Some(ACKNOWLEDGED.into()), None))
        );
        assert!(check_answer(&i, Some("No".into()), None).is_err());

        let mut f = choose();
        f.kind = AskKind::Form;
        f.fields = vec![
            field("n", "number", true, None),
            field("ok", "boolean", false, None),
        ];
        assert!(check_answer(&f, None, Some(json!({"n": "1"}))).is_err());
        assert!(check_answer(&f, None, Some(json!({"ok": true}))).is_err());
        assert_eq!(
            check_answer(&f, None, Some(json!({"n": 2, "ok": null}))),
            Ok((None, Some(json!({"n": 2}))))
        );
    }

    #[test]
    fn dedupe_titles_and_keys() {
        assert_eq!(normalize_title("  Merge PR #12? "), "merge pr 12");
        let mut a = choose();
        let mut b = choose();
        b.title = "which COLOUR".into();
        assert!(is_duplicate(&a, &b));
        a.dedupe_key = Some("k1".into());
        assert!(is_duplicate(&a, &b), "one key missing: titles decide");
        b.dedupe_key = Some("k2".into());
        assert!(!is_duplicate(&a, &b), "different keys are different asks");
        b.title = "Something else".into();
        b.dedupe_key = Some("k1".into());
        assert!(is_duplicate(&a, &b), "same key, different title");
    }

    #[test]
    fn prefix_checks_never_slice_inside_a_character() {
        assert!(starts_with_ignore_ascii_case("HTTPS://x", "https://"));
        assert!(
            !starts_with_ignore_ascii_case("1234567éx", "https://"),
            "byte 8 is inside 'é'"
        );
        assert!(!starts_with_ignore_ascii_case(
            "Notes/Résumé.md",
            "claude://"
        ));
        assert!(!starts_with_ignore_ascii_case("é", "https://"));
        assert!(!starts_with_ignore_ascii_case("", "x"));
        assert_eq!(
            https_lowercase_scheme("HTTPS://ntfy.sh/Topic").as_deref(),
            Some("https://ntfy.sh/Topic")
        );
        assert_eq!(
            https_lowercase_scheme("https://ntfy.sh/tópico").as_deref(),
            Some("https://ntfy.sh/tópico")
        );
        for bad in [
            "http://ntfy.sh/x",
            "https://",
            "https:///x",
            "1234567é/x",
            "ntfy.sh",
        ] {
            assert_eq!(https_lowercase_scheme(bad), None, "{bad:?}");
        }
        // check_link's own scheme test lower-cases first, so non-ASCII paths pass it as plain paths.
        assert_eq!(check_link("Notes/Résumé.md"), Ok(()));
        assert_eq!(check_link("1234567éx"), Ok(()));
    }

    #[test]
    fn a_default_waits_for_the_deadline_and_the_grace_after_shown() {
        assert_eq!(
            default_due_ms(NOW + DAY_MS, NOW),
            NOW + DAY_MS,
            "shown long before the deadline"
        );
        assert_eq!(
            default_due_ms(NOW - DAY_MS, NOW),
            NOW + DEFAULT_GRACE_MS,
            "shown when already overdue"
        );
        assert_eq!(default_due_ms(NOW, NOW - 10 * DEFAULT_GRACE_MS), NOW);
        assert_eq!(default_due_ms(0, i64::MAX), i64::MAX, "no overflow");
    }

    #[test]
    fn toast_rules() {
        let a = validate_ask(&choose(), NOW).unwrap().0;
        assert!(toast_buttons_allowed(&a));
        let mut irreversible = a.clone();
        irreversible.reversible = false;
        assert!(!toast_buttons_allowed(&irreversible));
        let mut text = a.clone();
        text.kind = AskKind::Text;
        assert!(!toast_buttons_allowed(&text));
        assert!(
            !toast_worthy(&a, NOW + 3 * URGENT_MS, NOW),
            "P2 not due soon"
        );
        assert!(toast_worthy(&a, NOW + URGENT_MS, NOW), "P2 due within 2 h");
        let mut p1 = a.clone();
        p1.priority = Priority::P1;
        assert!(toast_worthy(&p1, NOW + 10 * URGENT_MS, NOW));
    }

    #[test]
    fn owner_names() {
        assert!(check_owner_name("compass-storefront").is_ok());
        assert!(check_owner_name("codex:C:/DEV/Nestbolt").is_ok());
        assert!(check_owner_name("").is_err());
        assert!(check_owner_name("has space").is_err());
        assert!(check_owner_name("session:abc").is_err());
        assert!(check_owner_name(&"x".repeat(65)).is_err());
    }
}
