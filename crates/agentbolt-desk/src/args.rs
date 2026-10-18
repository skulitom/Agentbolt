//! `agentbolt-desk` arguments: the window, the protocol handler (`--url`), and the headless test answer.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// Open the Desk window.
    Window,
    /// The URL protocol handler: `<protocol>://answer?t=<token>` or `<protocol>://open`.
    Url(String),
    /// Test channels only: answer one ask through the Desk pipe and exit.
    Answer { ask_id: String, choice: Option<String>, values: Option<String>, note: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub channel: Option<String>,
    pub mode: Mode,
}

pub const USAGE: &str = "\
agentbolt-desk [--channel NAME]                                  open the Desk
agentbolt-desk [--channel NAME] --url <protocol>://answer?t=TOKEN    answer from a notification button
agentbolt-desk --channel NAME --answer ASK_ID --choice TEXT [--note N] [--values JSON]
                                                                 test channels only: answer and exit";

pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Args, String> {
    let items: Vec<String> = args.into_iter().collect();
    let mut i = 0;
    let (mut channel, mut url, mut answer, mut choice, mut values, mut note) = (None, None, None, None, None, None);
    while i < items.len() {
        let (flag, inline) = match items[i].split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_string(), Some(v.to_string())),
            _ => (items[i].clone(), None),
        };
        i += 1;
        let mut value = || -> Result<String, String> {
            if let Some(v) = inline.clone() {
                return Ok(v);
            }
            let v = items.get(i).cloned().ok_or_else(|| format!("{flag} needs a value"))?;
            i += 1;
            Ok(v)
        };
        match flag.as_str() {
            "--channel" => channel = Some(value()?),
            "--url" => url = Some(value()?),
            "--answer" => answer = Some(value()?),
            "--choice" => choice = Some(value()?),
            "--values" => values = Some(value()?),
            "--note" => note = Some(value()?),
            "--help" | "-h" => return Err(USAGE.to_string()),
            other => return Err(format!("'{other}' isn't an option\n\n{USAGE}")),
        }
    }
    let mode = match (url, answer) {
        (Some(_), Some(_)) => return Err("--url and --answer don't go together".into()),
        (Some(u), None) => Mode::Url(u),
        (None, Some(ask_id)) => {
            if choice.is_none() && values.is_none() {
                return Err("--answer needs --choice (or --values for a form)".into());
            }
            Mode::Answer { ask_id, choice, values, note }
        }
        (None, None) => {
            if choice.is_some() || values.is_some() || note.is_some() {
                return Err("--choice, --values and --note go with --answer".into());
            }
            Mode::Window
        }
    };
    Ok(Args { channel, mode })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(a: &[&str]) -> Result<Args, String> {
        parse(a.iter().map(|s| s.to_string()))
    }

    #[test]
    fn modes() {
        assert_eq!(p(&[]).unwrap(), Args { channel: None, mode: Mode::Window });
        assert_eq!(
            p(&["--channel", "t1", "--url", "agentbolt-t1://answer?t=abc"]).unwrap(),
            Args { channel: Some("t1".into()), mode: Mode::Url("agentbolt-t1://answer?t=abc".into()) }
        );
        assert_eq!(
            p(&["--channel=t1", "--answer", "a1", "--choice", "Ship", "--note", "ok"]).unwrap().mode,
            Mode::Answer { ask_id: "a1".into(), choice: Some("Ship".into()), values: None, note: Some("ok".into()) }
        );
    }

    #[test]
    fn mistakes_are_refused() {
        assert!(p(&["--answer", "a1"]).unwrap_err().contains("--choice"));
        assert!(p(&["--choice", "x"]).unwrap_err().contains("go with --answer"));
        assert!(p(&["--url", "u", "--answer", "a", "--choice", "c"]).is_err());
        assert!(p(&["--channel"]).unwrap_err().contains("needs a value"));
        assert!(p(&["--bogus"]).unwrap_err().contains("isn't an option"));
    }
}
