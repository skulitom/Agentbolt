//! Coordinates a binary replacement with the exact installed processes, across Windows sessions.

use agentbolt_core::channel::Names;
use agentbolt_ipc::desk_control::{Op, Prepared, endpoint};
use agentbolt_ipc::windows_util::{RunningImage, running_images};
use agentbolt_ipc::{Client, Pipe};
use anyhow::{Context, bail};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

struct Desk {
    process: RunningImage,
    client: Client,
    prepared: Prepared,
    finished: bool,
}

pub struct Upgrade {
    rt: tokio::runtime::Runtime,
    names: Names,
    directory: PathBuf,
    transaction: String,
    desks: Vec<Desk>,
    daemons: Vec<RunningImage>,
}

fn images(dir: &Path, stem: &str) -> Vec<PathBuf> {
    let mut paths = vec![dir.join(stem)];
    if let Ok(entries) = std::fs::read_dir(dir) {
        paths.extend(entries.flatten().filter_map(|e| {
            let name = e.file_name();
            let text = name.to_string_lossy().to_ascii_lowercase();
            (text.starts_with(&format!("{stem}.old-")) && crate::install::is_old_copy(&text))
                .then(|| e.path())
        }));
    }
    paths
}

impl Upgrade {
    pub fn prepare(names: &Names, directory: &Path) -> anyhow::Result<Self> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        // An opaque transaction is coordination, not answer authority. Use the profile's protected
        // instance lock to serialize installers; the per-process endpoint verifies its server image.
        let transaction = format!(
            "{:016x}{:016x}",
            std::process::id(),
            time::OffsetDateTime::now_utc().unix_timestamp_nanos() as u64
        );
        let mut upgrade = Self {
            rt,
            names: names.clone(),
            directory: directory.to_path_buf(),
            transaction,
            desks: Vec::new(),
            daemons: running_images(&images(directory, crate::install::DAEMON_EXE))?,
        };
        for process in running_images(&images(directory, crate::install::DESK_EXE))? {
            if process.exited()? {
                continue;
            }
            let outcome = upgrade.rt.block_on(async {
                let mut client = Client::connect_to(
                    &endpoint(names, process.pid),
                    Pipe::Mcp,
                    std::slice::from_ref(&process.image),
                )
                .await?;
                let response = tokio::time::timeout(
                    Duration::from_secs(35),
                    client.call(&Op::Prepare {
                        transaction: upgrade.transaction.clone(),
                    }),
                )
                .await??;
                let prepared: Prepared = serde_json::from_value(response)?;
                anyhow::ensure!(
                    prepared.lifecycle_version == 1,
                    "unsupported Desk lifecycle version"
                );
                anyhow::ensure!(
                    prepared.pid == process.pid && prepared.session == process.session,
                    "the Desk identity changed while preparing"
                );
                Ok::<_, anyhow::Error>((client, prepared))
            });
            match outcome {
                Ok((client, prepared)) => upgrade.desks.push(Desk {
                    process,
                    client,
                    prepared,
                    finished: false,
                }),
                Err(_) if process.exited()? => continue,
                Err(error) => bail!(
                    "Desk pid {} could not save and prepare for this upgrade: {error}. No programs were replaced. For an older Desk, finish or copy its unsent drafts, Quit Agentbolt from the tray, and retry.",
                    process.pid
                ),
            }
        }
        Ok(upgrade)
    }

    /// Renew after copying in case a slow disk consumed much of the pause lease. Retained pipe
    /// connections remain valid even though the verified old executable was renamed aside.
    pub fn renew(&mut self) -> anyhow::Result<()> {
        for desk in &mut self.desks {
            if desk.process.exited()? {
                continue;
            }
            self.rt
                .block_on(async {
                    tokio::time::timeout(
                        Duration::from_secs(35),
                        desk.client.call(&Op::Prepare {
                            transaction: self.transaction.clone(),
                        }),
                    )
                    .await??;
                    Ok::<_, anyhow::Error>(())
                })
                .context("renewing the Desk's saved upgrade pause")?;
        }
        // A new window in another session must not silently be killed by stopping its daemon.
        for process in running_images(&images(&self.directory, crate::install::DESK_EXE))? {
            anyhow::ensure!(
                self.desks.iter().any(|d| d.process.pid == process.pid),
                "Another Desk opened during the upgrade (pid {}); retry after closing it",
                process.pid
            );
        }
        Ok(())
    }

    pub fn daemon_session(&self) -> Option<u32> {
        self.daemons.iter().map(|p| p.session).find(|s| *s > 0)
    }

    pub fn request_visible_warm_restore(&self) -> anyhow::Result<()> {
        for desk in &self.desks {
            if desk.prepared.warm && desk.prepared.visible {
                agentbolt_ipc::desk_control::request_reopen(&self.names, desk.prepared.session)?;
            }
        }
        Ok(())
    }

    pub fn wait_for_daemons(&self) -> anyhow::Result<()> {
        let started = Instant::now();
        loop {
            let old_alive = self.daemons.iter().any(|p| !p.exited().unwrap_or(false));
            let replacement = running_images(&images(&self.directory, crate::install::DAEMON_EXE))?;
            if !old_alive && replacement.is_empty() {
                return Ok(());
            }
            anyhow::ensure!(
                started.elapsed() < Duration::from_secs(10),
                "The old Agentbolt processes did not stop within 10 seconds; upgrade stopped rather than running two versions"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn finish(&mut self, reopen: bool) -> anyhow::Result<()> {
        for desk in &mut self.desks {
            if desk.process.exited()? {
                desk.finished = true;
                continue;
            }
            let op = if reopen && desk.prepared.visible {
                Op::Reopen {
                    transaction: self.transaction.clone(),
                }
            } else {
                Op::Close {
                    transaction: self.transaction.clone(),
                }
            };
            self.rt
                .block_on(async {
                    tokio::time::timeout(Duration::from_secs(10), desk.client.call(&op)).await??;
                    Ok::<_, anyhow::Error>(())
                })
                .context("closing/reopening the prepared Desk")?;
            desk.finished = true;
            let started = Instant::now();
            while !desk.process.exited()? {
                anyhow::ensure!(
                    started.elapsed() < Duration::from_secs(10),
                    "The prepared old Desk (pid {}) did not exit",
                    desk.process.pid
                );
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        if reopen {
            for desk in &self.desks {
                if desk.prepared.visible {
                    self.wait_for_visible_desk(desk.prepared.session)?;
                }
            }
        }
        Ok(())
    }

    fn wait_for_visible_desk(&self, session: u32) -> anyhow::Result<()> {
        let started = Instant::now();
        let expected = self.directory.join(crate::install::DESK_EXE);
        loop {
            for process in running_images(std::slice::from_ref(&expected))? {
                if process.session != session
                    || self.desks.iter().any(|d| d.process.pid == process.pid)
                {
                    continue;
                }
                let ready = self
                    .rt
                    .block_on(async {
                        let mut client = Client::connect_to(
                            &endpoint(&self.names, process.pid),
                            Pipe::Mcp,
                            std::slice::from_ref(&expected),
                        )
                        .await
                        .ok()?;
                        let result =
                            tokio::time::timeout(Duration::from_secs(2), client.call(&Op::Inspect))
                                .await
                                .ok()?
                                .ok()?;
                        let state: Prepared = serde_json::from_value(result).ok()?;
                        Some(state.pid == process.pid && state.session == session && state.visible)
                    })
                    .unwrap_or(false);
                if ready {
                    return Ok(());
                }
            }
            anyhow::ensure!(
                started.elapsed() < Duration::from_secs(15),
                "The updated Desk did not reopen in Windows session {session}; its drafts remain saved"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for Upgrade {
    fn drop(&mut self) {
        for desk in &mut self.desks {
            if !desk.finished && !desk.process.exited().unwrap_or(true) {
                let _ = self.rt.block_on(async {
                    tokio::time::timeout(
                        Duration::from_secs(2),
                        desk.client.call(&Op::Cancel {
                            transaction: self.transaction.clone(),
                        }),
                    )
                    .await
                });
            }
        }
    }
}
