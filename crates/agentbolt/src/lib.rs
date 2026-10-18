//! `agentbolt`: the console exe agents and Artem run. `agentbolt mcp` is the MCP stdio server; the other
//! commands install, start and inspect the daemon (PLAN.md, "How it's built").
//!
//! This crate has no GUI dependencies and builds on Linux, so `agentbolt mcp` can answer discovery and
//! `tools/list` there (Glama; Anode B-049). Everything that touches Windows is behind `cfg(windows)`.

pub mod cli;
pub mod daemon;
pub mod install;
pub mod server;
pub mod tools;

use agentbolt_core::channel::Names;
use agentbolt_core::model::{AskInput, AskKind, Evidence};
use agentbolt_core::protocol::{McpOp, RegisterResult, StatusResult};
use agentbolt_ipc::Pipe;
use anyhow::{Context, bail};
use serde_json::Value;

use cli::{AskArgs, Command};
use daemon::Daemon;

/// Ask the daemon for its status over the MCP pipe (no start attempt).
pub async fn status(names: &Names) -> anyhow::Result<StatusResult> {
    let mut c = agentbolt_ipc::Client::connect(names, Pipe::Mcp).await?;
    let v = c.call(&McpOp::Status).await?;
    Ok(serde_json::from_value(v)?)
}

pub fn status_line(s: &StatusResult) -> String {
    format!(
        "Agentbolt {} is running on channel {}: {} open, {} agent(s); the event chain is {}.",
        s.version,
        s.channel,
        s.open,
        s.agents,
        if s.chain_ok {
            "intact"
        } else {
            "BROKEN (check the Desk)"
        }
    )
}

fn runtime() -> anyhow::Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?)
}

fn names_for(channel: Option<&str>) -> anyhow::Result<Names> {
    cli::names(channel).map_err(anyhow::Error::msg)
}

/// The `AskInput` for `agentbolt ask`.
pub fn ask_input(a: &AskArgs) -> anyhow::Result<AskInput> {
    let kind = match &a.kind {
        Some(k) => tools::parse_kind(k).map_err(anyhow::Error::msg)?,
        None if !a.options.is_empty() => AskKind::Choose,
        None => AskKind::Text,
    };
    let default = match (&a.default, kind) {
        (Some(d), AskKind::Form) => {
            serde_json::from_str(d).context("--default for a form is a JSON object")?
        }
        (Some(d), _) => Value::String(d.clone()),
        (None, AskKind::Inform) => Value::Null,
        (None, _) => bail!("ask needs --default: your recommended answer"),
    };
    Ok(AskInput {
        kind,
        title: a.title.clone(),
        body: a.body.clone(),
        evidence: a
            .evidence
            .iter()
            .map(|(label, url)| Evidence {
                label: label.clone(),
                url: url.clone(),
            })
            .collect(),
        options: a.options.clone(),
        fields: Vec::new(),
        default,
        deadline: tools::parse_deadline(&a.deadline).map_err(anyhow::Error::msg)?,
        reversible: a.reversible,
        priority: tools::parse_priority(&a.priority).map_err(anyhow::Error::msg)?,
        p0_reason: a.p0_reason.clone(),
        dedupe_key: a.dedupe_key.clone(),
    })
}

async fn post_ask(names: Names, a: &AskArgs) -> anyhow::Result<Value> {
    let input = ask_input(a)?;
    let d = Daemon::new(names, Pipe::Mcp, false);
    let reg = d
        .call(&McpOp::Register {
            owner: Some(a.owner.clone()),
            client: "agentbolt-cli".into(),
            session_ref: None,
            cwd: std::env::current_dir()
                .ok()
                .map(|p| p.display().to_string()),
            doing: Some("agentbolt ask".into()),
            provisional_key: None,
        })
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let reg: RegisterResult = serde_json::from_value(reg).context("register result")?;
    d.call(&McpOp::Ask {
        handle: reg.handle,
        ask: input,
    })
    .await
    .map_err(|e| anyhow::anyhow!("{e}"))
}

/// Run one command. Returns the process exit code.
pub fn run<I: IntoIterator<Item = String>>(args: I) -> i32 {
    let cmd = match cli::parse(args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    match dispatch(cmd) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("agentbolt: {e:#}");
            1
        }
    }
}

fn dispatch(cmd: Command) -> anyhow::Result<()> {
    match cmd {
        Command::Help => println!("{}", cli::USAGE),
        Command::Version => println!("agentbolt {}", env!("CARGO_PKG_VERSION")),
        Command::Mcp(m) => {
            let names = names_for(m.channel.as_deref())?;
            runtime()?.block_on(server::serve(m, names))?;
        }
        Command::Status { channel } => {
            let names = names_for(channel.as_deref())?;
            let s = runtime()?.block_on(status(&names))?;
            println!("{}", status_line(&s));
        }
        Command::Ask(a) => {
            let names = names_for(a.channel.as_deref())?;
            let v = runtime()?.block_on(post_ask(names, &a))?;
            println!("{}", tools::text(&tools::ask_line(&v, None), &v));
        }
        Command::Clients { channel, yes } => clients(&names_for(channel.as_deref())?, yes)?,
        Command::Install {
            channel,
            from,
            no_start,
        } => {
            let names = names_for(channel.as_deref())?;
            let from = match from {
                Some(f) => f,
                None => std::env::current_exe()?
                    .parent()
                    .context("the exe has no folder")?
                    .to_path_buf(),
            };
            platform::install(&names, &from, !no_start)?;
        }
        Command::Uninstall { channel, purge } => {
            platform::uninstall(&names_for(channel.as_deref())?, purge)?
        }
        Command::Start { channel } => platform::start(&names_for(channel.as_deref())?)?,
    }
    Ok(())
}

fn clients(names: &Names, yes: bool) -> anyhow::Result<()> {
    let install = install::install_dir(names)?;
    let cmds = install::client_commands(names, &install);
    for (what, program, args) in &cmds {
        println!("{what}:\n  {}", install::shell_line(program, args));
    }
    if !yes {
        println!("\nRun `agentbolt clients --yes` to run the two register commands.");
        return Ok(());
    }
    for (what, program, args) in cmds.iter().take(2) {
        let Some(exe) = platform::find_program(program) else {
            println!("{what}: skipped, {program} isn't on PATH");
            continue;
        };
        let status = std::process::Command::new(&exe).args(args).status();
        match status {
            Ok(s) if s.success() => println!("{what}: done"),
            Ok(s) => println!("{what}: {program} exited with {s}"),
            Err(e) => println!("{what}: couldn't run {}: {e}", exe.display()),
        }
    }
    Ok(())
}

#[cfg(windows)]
mod platform {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use agentbolt_core::channel::Names;

    pub fn install(names: &Names, from: &Path, start: bool) -> anyhow::Result<()> {
        crate::install::install(names, from, start)
    }

    pub fn uninstall(names: &Names, purge: bool) -> anyhow::Result<()> {
        crate::install::uninstall(names, purge)
    }

    pub fn start(names: &Names) -> anyhow::Result<()> {
        crate::install::start_and_wait(names, Duration::from_secs(10))
    }

    /// `name.exe`, `.cmd` or `.bat` on PATH (npm installs CLIs as .cmd shims).
    pub fn find_program(name: &str) -> Option<PathBuf> {
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path).find_map(|dir| {
            ["exe", "cmd", "bat"]
                .iter()
                .map(|ext| dir.join(format!("{name}.{ext}")))
                .find(|p| p.is_file())
        })
    }
}

#[cfg(not(windows))]
mod platform {
    use std::path::{Path, PathBuf};

    use agentbolt_core::channel::Names;

    const ONLY_WINDOWS: &str =
        "this command is Windows-only for now (the sign-in task and the Desk are Windows programs)";

    pub fn install(_: &Names, _: &Path, _: bool) -> anyhow::Result<()> {
        anyhow::bail!(ONLY_WINDOWS)
    }

    pub fn uninstall(_: &Names, _: bool) -> anyhow::Result<()> {
        anyhow::bail!(ONLY_WINDOWS)
    }

    pub fn start(_: &Names) -> anyhow::Result<()> {
        anyhow::bail!(ONLY_WINDOWS)
    }

    pub fn find_program(name: &str) -> Option<PathBuf> {
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path)
            .map(|d| d.join(name))
            .find(|p| p.is_file())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask_args() -> AskArgs {
        AskArgs {
            channel: None,
            owner: "me".into(),
            kind: None,
            title: "Ship?".into(),
            body: String::new(),
            options: vec!["Ship".into(), "Wait".into()],
            default: Some("Ship".into()),
            deadline: "2026-10-20T18:00:00Z".into(),
            reversible: true,
            priority: "P1".into(),
            p0_reason: None,
            evidence: vec![],
            dedupe_key: None,
        }
    }

    #[test]
    fn cli_ask_builds_a_choose_ask() {
        let a = ask_input(&ask_args()).unwrap();
        assert_eq!(a.kind, AskKind::Choose);
        assert_eq!(a.default, Value::String("Ship".into()));
        let mut t = ask_args();
        t.options.clear();
        assert_eq!(ask_input(&t).unwrap().kind, AskKind::Text);
        t.default = None;
        assert!(ask_input(&t).unwrap_err().to_string().contains("--default"));
        let mut f = ask_args();
        f.kind = Some("form".into());
        f.default = Some("{\"n\": 3}".into());
        assert_eq!(ask_input(&f).unwrap().default, serde_json::json!({"n": 3}));
    }

    #[test]
    fn status_line_reads_well() {
        let s = StatusResult {
            version: "0.1.0".into(),
            channel: "t1".into(),
            open: 2,
            digest: 1,
            agents: 3,
            chain_ok: true,
        };
        assert_eq!(
            status_line(&s),
            "Agentbolt 0.1.0 is running on channel t1: 2 open, 3 agent(s); the event chain is intact."
        );
    }
}
