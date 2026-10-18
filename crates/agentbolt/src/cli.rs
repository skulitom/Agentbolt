//! Argument parsing for `agentbolt`. Hand-rolled (no clap) so the console exe stays small; every flag takes
//! `--flag value` or `--flag=value`.

use std::path::PathBuf;

use agentbolt_core::channel::Names;

/// The cap on one `desk_answers` wait unless `--max-wait-ms` says otherwise (PLAN.md: configurable per host).
pub const DEFAULT_MAX_WAIT_MS: u64 = 20_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpArgs {
    pub channel: Option<String>,
    /// The owner this server's agents ask as (bound by the host's config, PLAN.md rule 2).
    pub owner: Option<String>,
    /// The client's name, when the host's own `clientInfo` isn't the one wanted.
    pub client: Option<String>,
    pub max_wait_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskArgs {
    pub channel: Option<String>,
    pub owner: String,
    pub kind: Option<String>,
    pub title: String,
    pub body: String,
    pub options: Vec<String>,
    pub default: Option<String>,
    pub deadline: String,
    pub reversible: bool,
    pub priority: String,
    pub p0_reason: Option<String>,
    /// `label=url` pairs.
    pub evidence: Vec<(String, String)>,
    pub dedupe_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Mcp(McpArgs),
    Install {
        channel: Option<String>,
        from: Option<PathBuf>,
        no_start: bool,
    },
    Uninstall {
        channel: Option<String>,
        purge: bool,
    },
    Start {
        channel: Option<String>,
    },
    Status {
        channel: Option<String>,
    },
    Clients {
        channel: Option<String>,
        yes: bool,
    },
    Ask(Box<AskArgs>),
    Help,
    Version,
}

pub const USAGE: &str = "\
agentbolt: Artem's desk for agents (CLI and MCP server)

Usage:
  agentbolt mcp [--owner NAME] [--client NAME] [--max-wait-ms N]   MCP server on stdio
  agentbolt install [--from DIR] [--no-start]   copy the exes, register, create the sign-in task, start it
  agentbolt uninstall [--purge]                  remove the task, registry keys and install folder (--purge: data too)
  agentbolt start                                start the daemon through its sign-in task
  agentbolt status                               ask the daemon how it is
  agentbolt clients [--yes]                      print (or run) the Claude Code and Codex registration commands
  agentbolt ask --owner NAME --title T [--body B] [--option A]... [--default A] --deadline RFC3339
                [--reversible] [--priority P0|P1|P2] [--p0-reason R] [--kind K] [--evidence LABEL=URL]...
                [--dedupe-key K]
  agentbolt --version

Every command takes --channel NAME (default: $AGENTBOLT_CHANNEL, else main).";

struct Args {
    items: Vec<String>,
    pos: usize,
}

impl Args {
    /// The next flag and, for `--flag=value`, its inline value.
    fn next_flag(&mut self) -> Option<(String, Option<String>)> {
        let item = self.items.get(self.pos)?.clone();
        self.pos += 1;
        match item.split_once('=') {
            Some((flag, value)) if flag.starts_with("--") => {
                Some((flag.to_string(), Some(value.to_string())))
            }
            _ => Some((item, None)),
        }
    }

    fn value(&mut self, flag: &str, inline: Option<String>) -> Result<String, String> {
        if let Some(v) = inline {
            return Ok(v);
        }
        let v = self
            .items
            .get(self.pos)
            .cloned()
            .ok_or_else(|| format!("{flag} needs a value"))?;
        self.pos += 1;
        Ok(v)
    }
}

fn no_value(flag: &str, inline: &Option<String>) -> Result<(), String> {
    match inline {
        Some(_) => Err(format!("{flag} takes no value")),
        None => Ok(()),
    }
}

pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Command, String> {
    let mut items: Vec<String> = args.into_iter().collect();
    if items.is_empty() {
        return Ok(Command::Help);
    }
    let sub = items.remove(0);
    let mut a = Args { items, pos: 0 };
    match sub.as_str() {
        "help" | "--help" | "-h" => Ok(Command::Help),
        "version" | "--version" | "-V" => Ok(Command::Version),
        "mcp" => {
            let mut m = McpArgs {
                channel: None,
                owner: None,
                client: None,
                max_wait_ms: DEFAULT_MAX_WAIT_MS,
            };
            while let Some((flag, inline)) = a.next_flag() {
                match flag.as_str() {
                    "--channel" => m.channel = Some(a.value(&flag, inline)?),
                    "--owner" => m.owner = Some(nonempty(&flag, a.value(&flag, inline)?)?),
                    "--client" => m.client = Some(nonempty(&flag, a.value(&flag, inline)?)?),
                    "--max-wait-ms" => {
                        let v = a.value(&flag, inline)?;
                        m.max_wait_ms = v
                            .parse()
                            .map_err(|_| format!("--max-wait-ms takes milliseconds, not '{v}'"))?;
                    }
                    other => return Err(unknown(other, "mcp")),
                }
            }
            Ok(Command::Mcp(m))
        }
        "install" => {
            let (mut channel, mut from, mut no_start) = (None, None, false);
            while let Some((flag, inline)) = a.next_flag() {
                match flag.as_str() {
                    "--channel" => channel = Some(a.value(&flag, inline)?),
                    "--from" => from = Some(PathBuf::from(a.value(&flag, inline)?)),
                    "--no-start" => {
                        no_value(&flag, &inline)?;
                        no_start = true
                    }
                    other => return Err(unknown(other, "install")),
                }
            }
            Ok(Command::Install {
                channel,
                from,
                no_start,
            })
        }
        "uninstall" => {
            let (mut channel, mut purge) = (None, false);
            while let Some((flag, inline)) = a.next_flag() {
                match flag.as_str() {
                    "--channel" => channel = Some(a.value(&flag, inline)?),
                    "--purge" => {
                        no_value(&flag, &inline)?;
                        purge = true
                    }
                    other => return Err(unknown(other, "uninstall")),
                }
            }
            Ok(Command::Uninstall { channel, purge })
        }
        "start" | "status" => {
            let mut channel = None;
            while let Some((flag, inline)) = a.next_flag() {
                match flag.as_str() {
                    "--channel" => channel = Some(a.value(&flag, inline)?),
                    other => return Err(unknown(other, &sub)),
                }
            }
            Ok(if sub == "start" {
                Command::Start { channel }
            } else {
                Command::Status { channel }
            })
        }
        "clients" => {
            let (mut channel, mut yes) = (None, false);
            while let Some((flag, inline)) = a.next_flag() {
                match flag.as_str() {
                    "--channel" => channel = Some(a.value(&flag, inline)?),
                    "--yes" => {
                        no_value(&flag, &inline)?;
                        yes = true
                    }
                    other => return Err(unknown(other, "clients")),
                }
            }
            Ok(Command::Clients { channel, yes })
        }
        "ask" => parse_ask(&mut a).map(|x| Command::Ask(Box::new(x))),
        other => Err(format!("unknown command '{other}'\n\n{USAGE}")),
    }
}

fn parse_ask(a: &mut Args) -> Result<AskArgs, String> {
    let mut r = AskArgs {
        channel: None,
        owner: String::new(),
        kind: None,
        title: String::new(),
        body: String::new(),
        options: Vec::new(),
        default: None,
        deadline: String::new(),
        reversible: false,
        priority: "P1".into(),
        p0_reason: None,
        evidence: Vec::new(),
        dedupe_key: None,
    };
    while let Some((flag, inline)) = a.next_flag() {
        match flag.as_str() {
            "--channel" => r.channel = Some(a.value(&flag, inline)?),
            "--owner" => r.owner = a.value(&flag, inline)?,
            "--kind" => r.kind = Some(a.value(&flag, inline)?),
            "--title" => r.title = a.value(&flag, inline)?,
            "--body" => r.body = a.value(&flag, inline)?,
            "--option" => r.options.push(a.value(&flag, inline)?),
            "--default" => r.default = Some(a.value(&flag, inline)?),
            "--deadline" => r.deadline = a.value(&flag, inline)?,
            "--reversible" => {
                no_value(&flag, &inline)?;
                r.reversible = true
            }
            "--priority" => r.priority = a.value(&flag, inline)?.to_ascii_uppercase(),
            "--p0-reason" => r.p0_reason = Some(a.value(&flag, inline)?),
            "--dedupe-key" => r.dedupe_key = Some(a.value(&flag, inline)?),
            "--evidence" => {
                let v = a.value(&flag, inline)?;
                let (label, url) = v
                    .split_once('=')
                    .ok_or_else(|| format!("--evidence takes LABEL=URL, not '{v}'"))?;
                r.evidence.push((label.to_string(), url.to_string()));
            }
            other => return Err(unknown(other, "ask")),
        }
    }
    for (name, value) in [
        ("--owner", &r.owner),
        ("--title", &r.title),
        ("--deadline", &r.deadline),
    ] {
        if value.trim().is_empty() {
            return Err(format!("ask needs {name}"));
        }
    }
    if !matches!(r.priority.as_str(), "P0" | "P1" | "P2") {
        return Err(format!(
            "--priority takes P0, P1 or P2, not '{}'",
            r.priority
        ));
    }
    Ok(r)
}

fn nonempty(flag: &str, v: String) -> Result<String, String> {
    if v.trim().is_empty() {
        Err(format!("{flag} needs a value"))
    } else {
        Ok(v)
    }
}

fn unknown(flag: &str, sub: &str) -> String {
    format!("'{flag}' isn't an option of `agentbolt {sub}`\n\n{USAGE}")
}

/// The channel: `--channel`, else `AGENTBOLT_CHANNEL`, else `main`.
pub fn names(channel: Option<&str>) -> Result<Names, String> {
    match channel {
        Some(c) => Names::for_channel(c.trim()),
        None => Names::from_env(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Command, String> {
        parse(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn mcp_flags_parse_in_both_spellings() {
        let c = p(&[
            "mcp",
            "--owner",
            "compass-storefront",
            "--client=codex",
            "--max-wait-ms",
            "90000",
        ])
        .unwrap();
        assert_eq!(
            c,
            Command::Mcp(McpArgs {
                channel: None,
                owner: Some("compass-storefront".into()),
                client: Some("codex".into()),
                max_wait_ms: 90_000,
            })
        );
        let Command::Mcp(m) = p(&["mcp"]).unwrap() else {
            panic!()
        };
        assert_eq!(m.max_wait_ms, DEFAULT_MAX_WAIT_MS);
        assert_eq!(m.owner, None);
    }

    #[test]
    fn bad_flags_are_refused_with_a_reason() {
        assert!(
            p(&["mcp", "--owner"])
                .unwrap_err()
                .contains("needs a value")
        );
        assert!(
            p(&["mcp", "--max-wait-ms", "soon"])
                .unwrap_err()
                .contains("milliseconds")
        );
        assert!(
            p(&["mcp", "--bogus"])
                .unwrap_err()
                .contains("isn't an option")
        );
        assert!(p(&["frobnicate"]).unwrap_err().contains("unknown command"));
        assert!(
            p(&["uninstall", "--purge=yes"])
                .unwrap_err()
                .contains("takes no value")
        );
    }

    #[test]
    fn install_uninstall_clients_parse() {
        assert_eq!(
            p(&[
                "install",
                "--channel",
                "t1",
                "--from",
                "C:\\b",
                "--no-start"
            ])
            .unwrap(),
            Command::Install {
                channel: Some("t1".into()),
                from: Some(PathBuf::from("C:\\b")),
                no_start: true
            }
        );
        assert_eq!(
            p(&["uninstall", "--channel=t1", "--purge"]).unwrap(),
            Command::Uninstall {
                channel: Some("t1".into()),
                purge: true
            }
        );
        assert_eq!(
            p(&["clients"]).unwrap(),
            Command::Clients {
                channel: None,
                yes: false
            }
        );
        assert_eq!(
            p(&["status", "--channel", "x"]).unwrap(),
            Command::Status {
                channel: Some("x".into())
            }
        );
        assert_eq!(p(&[]).unwrap(), Command::Help);
        assert_eq!(p(&["--version"]).unwrap(), Command::Version);
    }

    #[test]
    fn ask_needs_owner_title_and_deadline() {
        let c = p(&[
            "ask",
            "--owner",
            "me",
            "--title",
            "Ship it?",
            "--option",
            "Ship",
            "--option",
            "Wait",
            "--default",
            "Ship",
            "--deadline",
            "2026-10-20T18:00:00Z",
            "--reversible",
            "--priority",
            "p0",
            "--evidence",
            "PR=https://github.com/x/y/pull/1",
        ])
        .unwrap();
        let Command::Ask(a) = c else { panic!() };
        assert_eq!(a.options, vec!["Ship", "Wait"]);
        assert_eq!(a.priority, "P0");
        assert!(a.reversible);
        assert_eq!(
            a.evidence,
            vec![(
                "PR".to_string(),
                "https://github.com/x/y/pull/1".to_string()
            )]
        );
        assert!(
            p(&["ask", "--title", "x", "--deadline", "d"])
                .unwrap_err()
                .contains("--owner")
        );
        assert!(
            p(&["ask", "--owner", "o", "--title", "x"])
                .unwrap_err()
                .contains("--deadline")
        );
        assert!(
            p(&[
                "ask",
                "--owner",
                "o",
                "--title",
                "x",
                "--deadline",
                "d",
                "--priority",
                "P9"
            ])
            .unwrap_err()
            .contains("P0, P1 or P2")
        );
    }

    #[test]
    fn channel_flag_beats_the_environment() {
        assert_eq!(names(Some("t1")).unwrap().channel, "t1");
        assert!(names(Some("Bad Name")).is_err());
    }
}
