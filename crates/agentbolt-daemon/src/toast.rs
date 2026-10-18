//! Silent toasts under the channel's AppUserModelID (PLAN.md, "Toasts (v1)").
//!
//! The XML is built here because tauri-winrt-notification 0.8 can't make protocol-activated buttons:
//! - the toast body opens the Desk: `<protocol>://open`;
//! - each button carries its own single-use token: `<protocol>://answer?t=<token>`, handled by the GUI-subsystem
//!   `agentbolt-desk.exe`, so no console window flashes;
//! - `<audio silent="true"/>` always (alarm-like sounds agitate Artem).
//!
//! Ask titles and bodies are untrusted text: escaped, control characters dropped, the body cut to 200 characters.
//! Every cut counts characters, never bytes, so non-ASCII text can't split inside a character.

use agentbolt_core::channel::Names;

pub const MAX_BODY: usize = 200;
const MAX_TITLE: usize = 120;
const MAX_LABEL: usize = 40;
/// Windows shows at most five actions.
const MAX_BUTTONS: usize = 5;

/// Plain text: control characters become spaces, then cut to `max` characters with an ellipsis.
pub fn plain(text: &str, max: usize) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if cleaned.chars().count() <= max {
        cleaned
    } else {
        let mut cut: String = cleaned.chars().take(max - 1).collect();
        cut.push('…');
        cut
    }
}

fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

/// Percent-encode everything but unreserved URL characters (tokens are `[0-9a-f.]`, so this is a guard only).
fn url_component(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn open_url(names: &Names) -> String {
    format!("{}://open", names.protocol)
}

pub fn answer_url(names: &Names, token: &str) -> String {
    format!("{}://answer?t={}", names.protocol, url_component(token))
}

/// The buttons' labels as the toast shows them (cut to 40 characters), or None when two of them would read the
/// same: options may be 120 characters, and two that differ only past the cut can't be told apart, so such a
/// toast gets no buttons (its body click still opens the Desk).
pub fn button_labels(buttons: &[(String, String)]) -> Option<Vec<String>> {
    let labels: Vec<String> = buttons
        .iter()
        .take(MAX_BUTTONS)
        .map(|(label, _)| plain(label, MAX_LABEL))
        .collect();
    let mut seen = std::collections::HashSet::new();
    labels
        .iter()
        .all(|l| !l.is_empty() && seen.insert(l.as_str()))
        .then_some(labels)
}

/// The toast XML for one ask. `buttons` are (label, token) pairs from `Effect::Toast`.
pub fn xml(names: &Names, title: &str, body: &str, buttons: &[(String, String)]) -> String {
    let mut x = String::new();
    x.push_str(&format!(
        r#"<toast activationType="protocol" launch="{}">"#,
        xml_escape(&open_url(names))
    ));
    x.push_str(r#"<visual><binding template="ToastGeneric">"#);
    x.push_str(&format!(
        r#"<text hint-maxLines="2">{}</text>"#,
        xml_escape(&plain(title, MAX_TITLE))
    ));
    let body = plain(body, MAX_BODY);
    if !body.is_empty() {
        x.push_str(&format!("<text>{}</text>", xml_escape(&body)));
    }
    x.push_str("</binding></visual>");
    x.push_str(r#"<audio silent="true"/>"#);
    if let Some(labels) = button_labels(buttons).filter(|l| !l.is_empty()) {
        x.push_str("<actions>");
        for (label, (_, token)) in labels.iter().zip(buttons) {
            x.push_str(&format!(
                r#"<action content="{}" activationType="protocol" arguments="{}"/>"#,
                xml_escape(label),
                xml_escape(&answer_url(names, token))
            ));
        }
        x.push_str("</actions>");
    }
    x.push_str("</toast>");
    x
}

/// Show the toast. Tagged with the ask id so a later version could withdraw it.
#[cfg(windows)]
pub fn show(
    names: &Names,
    ask_id: &str,
    title: &str,
    body: &str,
    buttons: &[(String, String)],
) -> Result<(), String> {
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
    use windows::core::HSTRING;

    let run = || -> windows::core::Result<()> {
        let doc = XmlDocument::new()?;
        doc.LoadXml(&HSTRING::from(xml(names, title, body, buttons)))?;
        let toast = ToastNotification::CreateToastNotification(&doc)?;
        let tag: String = ask_id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .take(64)
            .collect();
        if !tag.is_empty() {
            toast.SetTag(&HSTRING::from(tag))?;
            toast.SetGroup(&HSTRING::from("asks"))?;
        }
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(&names.aumid))?
            .Show(&toast)
    };
    run().map_err(|e| format!("toast failed: {e}"))
}

/// Whether Windows will show toasts for this channel's AUMID (for the start-up log). Shows nothing.
#[cfg(windows)]
pub fn setting(names: &Names) -> String {
    use windows::UI::Notifications::{NotificationSetting, ToastNotificationManager};
    use windows::core::HSTRING;
    match ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(&names.aumid))
        .and_then(|n| n.Setting())
    {
        Ok(NotificationSetting::Enabled) => "enabled".into(),
        Ok(NotificationSetting::DisabledForApplication) => {
            "turned off for Agentbolt in Settings".into()
        }
        Ok(NotificationSetting::DisabledForUser) => "turned off for this user".into(),
        Ok(NotificationSetting::DisabledByGroupPolicy) => "turned off by group policy".into(),
        Ok(NotificationSetting::DisabledByManifest) => "turned off by the manifest".into(),
        Ok(other) => format!("setting {}", other.0),
        Err(e) => format!("unknown ({e})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Names {
        Names::for_channel("ipctest-toast").unwrap()
    }

    #[test]
    fn the_toast_is_silent_with_protocol_buttons() {
        let n = names();
        let buttons = vec![
            ("Ship it".to_string(), "ab12.cd34".to_string()),
            ("Wait".to_string(), "ef56.0789".to_string()),
        ];
        let x = xml(&n, "Merge PR #12?", "Tests pass.", &buttons);
        assert!(x.contains(r#"<audio silent="true"/>"#));
        assert!(x.contains(r#"launch="agentbolt-ipctest-toast://open""#));
        assert!(x.contains(r#"<action content="Ship it" activationType="protocol" arguments="agentbolt-ipctest-toast://answer?t=ab12.cd34"/>"#));
        assert!(x.contains(r#"arguments="agentbolt-ipctest-toast://answer?t=ef56.0789""#));
        assert_eq!(x.matches("<action ").count(), 2);
    }

    #[test]
    fn untrusted_text_is_escaped_and_cut() {
        let n = names();
        let hostile =
            r#"</text><action content="x" activationType="protocol" arguments="evil://"/>"#;
        let long = format!("{hostile} {}", "word ".repeat(100));
        let buttons = vec![(r#"A"&<b>"#.to_string(), "t&x=1".to_string())];
        let x = xml(&n, hostile, &long, &buttons);
        assert!(!x.contains("evil://\""), "no injected markup");
        assert_eq!(x.matches("<action ").count(), 1);
        assert!(x.contains("content=\"A&quot;&amp;&lt;b&gt;\""));
        assert!(x.contains("answer?t=t%26x%3D1"));
        let body = plain(&long, MAX_BODY);
        assert_eq!(body.chars().count(), MAX_BODY);
        assert!(body.ends_with('…'));
        assert_eq!(plain("a\r\n\tb\u{7}c", 50), "a b c");
        let six: Vec<_> = (0..6).map(|i| (format!("o{i}"), format!("{i}"))).collect();
        assert_eq!(
            xml(&n, "t", "", &six).matches("<action ").count(),
            5,
            "at most five buttons"
        );
        assert!(
            !xml(&n, "t", "", &[]).contains("<actions>"),
            "no buttons: the body click opens the Desk"
        );
    }

    #[test]
    fn non_ascii_text_is_cut_by_characters() {
        let n = names();
        let title = format!("Résumé {}", "é".repeat(200));
        let body = "Ünïcödé ".repeat(60);
        // 39 'é' then a two-byte 'ß' at byte 78-79: a byte cut at 40 would land inside a character.
        let long = format!("{}ßxyz", "é".repeat(39));
        let buttons = vec![
            (long.clone(), "aa.bb".to_string()),
            ("Ja, bitte 🙂".to_string(), "cc.dd".to_string()),
        ];
        let x = xml(&n, &title, &body, &buttons);
        let labels = button_labels(&buttons).unwrap();
        assert_eq!(labels[0].chars().count(), MAX_LABEL);
        assert!(
            labels[0].starts_with(&"é".repeat(39)) && labels[0].ends_with('…'),
            "{}",
            labels[0]
        );
        assert_eq!(labels[1], "Ja, bitte 🙂");
        assert!(x.contains(&format!("content=\"{}\"", labels[0])));
        assert_eq!(plain(&title, MAX_TITLE).chars().count(), MAX_TITLE);
        assert!(plain(&body, MAX_BODY).starts_with("Ünïcödé Ünïcödé"));
        assert_eq!(plain("1234567éx", 8), "1234567…");
    }

    #[test]
    fn labels_that_read_the_same_after_the_cut_get_no_buttons() {
        let n = names();
        let stem = "Merge the branch and then deploy to the"; // 39 characters
        let a = format!("{stem} staging servers first");
        let b = format!("{stem} production servers right away");
        let buttons = vec![
            (a, "aa.bb".to_string()),
            (b, "cc.dd".to_string()),
            ("Wait".to_string(), "ee.ff".to_string()),
        ];
        assert_eq!(
            button_labels(&buttons),
            None,
            "two labels cut to the same text"
        );
        let x = xml(&n, "Deploy?", "", &buttons);
        assert!(
            !x.contains("<actions>") && !x.contains("answer?t="),
            "no buttons at all: {x}"
        );
        assert!(
            x.contains(r#"launch="agentbolt-ipctest-toast://open""#),
            "the body still opens the Desk"
        );
        let distinct = vec![
            ("Ship".to_string(), "aa.bb".to_string()),
            ("Wait".to_string(), "cc.dd".to_string()),
        ];
        assert_eq!(
            button_labels(&distinct),
            Some(vec!["Ship".to_string(), "Wait".to_string()])
        );
    }

    /// The XML is valid for Windows' own parser and makes a ToastNotification (nothing is shown).
    #[cfg(windows)]
    #[test]
    fn windows_accepts_the_toast_xml() {
        use windows::Data::Xml::Dom::XmlDocument;
        use windows::UI::Notifications::ToastNotification;
        use windows::core::HSTRING;
        let n = names();
        let buttons = vec![
            ("Yes".to_string(), "aa.bb".to_string()),
            ("No <way>".to_string(), "cc.dd".to_string()),
        ];
        let doc = XmlDocument::new().unwrap();
        doc.LoadXml(&HSTRING::from(xml(
            &n,
            "Title & <more>",
            "Body \"quoted\"",
            &buttons,
        )))
        .unwrap();
        ToastNotification::CreateToastNotification(&doc).unwrap();
        let actions = doc.GetElementsByTagName(&HSTRING::from("action")).unwrap();
        assert_eq!(actions.Length().unwrap(), 2);
    }
}
