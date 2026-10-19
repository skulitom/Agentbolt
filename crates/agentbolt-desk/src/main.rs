#![cfg_attr(windows, windows_subsystem = "windows")]
//! agentbolt-desk: Artem's Desk window, and the `agentbolt://` handler that toast buttons open.
//! A GUI-subsystem program, so a toast click flashes no console window (PLAN.md, Toasts).

mod app;
mod args;
mod chat;
mod drafts;
mod os;
mod resident;
mod startup;
mod theme;
mod upgrade;
mod view;
mod worker;

use agentbolt_core::channel::Names;
use agentbolt_core::protocol::DeskOp;
use agentbolt_ipc::{Client, Pipe};

use args::Mode;
use view::UrlAction;

fn names(channel: Option<&str>) -> Result<Names, String> {
    match channel {
        Some(c) => Names::for_channel(c.trim()),
        None => Names::from_env(),
    }
}

/// One Desk-pipe op, from a fresh connection.
fn desk_call(names: &Names, op: &DeskOp) -> Result<serde_json::Value, String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    rt.block_on(async {
        let mut c = Client::connect(names, Pipe::Desk)
            .await
            .map_err(|e| e.to_string())?;
        c.call(op).await.map_err(|e| e.to_string())
    })
}

fn main() {
    startup::init();
    let parsed = match args::parse(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let names = match names(parsed.channel.as_deref()) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("{e}");
            if matches!(parsed.mode, Mode::Url(_)) {
                os::message("Agentbolt", &e);
            }
            std::process::exit(2);
        }
    };
    if names.is_main() && matches!(parsed.mode, Mode::Answer { .. }) {
        eprintln!(
            "agentbolt-desk --answer is for test channels only; pass --channel <test channel>."
        );
        std::process::exit(2);
    }
    #[cfg(windows)]
    match agentbolt_ipc::windows_util::InstanceLock::startup(&names) {
        Ok(Some(gate)) => drop(gate),
        _ => {
            // Do not enter the newly copied build before the installer has committed it. In
            // particular a warm preload must not create a second UI during draft preparation.
            eprintln!("Agentbolt is updating; open the Desk again when the update finishes.");
            std::process::exit(1);
        }
    }
    let code = match parsed.mode {
        Mode::Window => window(names, false),
        Mode::Warm => window(names, true),
        Mode::PhoneReply(id) => match desk_call(&names, &DeskOp::PhoneReply { id }) {
            Ok(_) => 0,
            Err(_) => 1,
        },
        Mode::Url(url) => match view::parse_url(&url, &names.protocol) {
            Ok(UrlAction::Open) => window(names, false),
            Ok(UrlAction::Answer { token }) => {
                match desk_call(&names, &DeskOp::AnswerToken { token }) {
                    Ok(_) => 0,
                    Err(e) => {
                        os::message(
                            "Agentbolt",
                            &format!(
                                "That answer didn't go through: {e}\n\nOpen the Desk to answer it there."
                            ),
                        );
                        1
                    }
                }
            }
            Err(e) => {
                os::message("Agentbolt", &e);
                2
            }
        },
        Mode::Answer {
            ask_id,
            choice,
            values,
            note,
        } => {
            let values = match values.map(|v| serde_json::from_str(&v)) {
                None => None,
                Some(Ok(v)) => Some(v),
                Some(Err(e)) => {
                    eprintln!("--values must be JSON: {e}");
                    std::process::exit(2);
                }
            };
            match desk_call(
                &names,
                &DeskOp::Answer {
                    one_click: None,
                    ask_id,
                    choice,
                    values,
                    note,
                },
            ) {
                Ok(v) => {
                    println!("{v}");
                    0
                }
                Err(e) => {
                    eprintln!("{e}");
                    1
                }
            }
        }
    };
    std::process::exit(code);
}

fn window(names: Names, warm: bool) -> i32 {
    match app::run(names, warm) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            os::message("Agentbolt", &e.to_string());
            1
        }
    }
}
