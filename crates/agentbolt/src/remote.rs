//! MCP-only SSH relay. Agentbolt never opens a network listener or accepts human answers here.
//! SSH handles encryption, account authentication and known-host verification; setup is explicit.
use crate::daemon::{CallError, Daemon};
use agentbolt_core::{
    channel::Names,
    protocol::{McpOp, RemoteRegistration, Request, Response},
};
use agentbolt_ipc::Pipe;
use serde::Serialize;
use serde_json::Value;
use std::{path::PathBuf, process::Stdio, sync::Mutex, time::Duration};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout},
    sync::{Semaphore, SemaphorePermit},
};

const MAX_REQUEST: usize = 1 << 20;
const MAX_RESPONSE: usize = 16 << 20;
const MAX_CONNECTIONS: usize = 4;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Shell {
    #[default]
    Sh,
    Cmd,
    PowerShell,
}

impl Shell {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "sh" => Ok(Self::Sh),
            "cmd" => Ok(Self::Cmd),
            "powershell" => Ok(Self::PowerShell),
            _ => Err("--remote-shell must be sh, cmd or powershell".into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub host: String,
    pub machine: String,
    pub executable: String,
    /// The SSH account's command shell, which is independent of the local client's OS.
    pub shell: Shell,
    /// Explicit host-owned SSH configuration, passed as one local argument; never sent remotely.
    pub config_file: Option<PathBuf>,
}

pub fn validate_machine(machine: &str) -> Result<(), String> {
    if machine.is_empty()
        || machine.len() > 64
        || !machine
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err("machine needs 1–64 letters, digits, dots, underscores or hyphens".into());
    }
    Ok(())
}
impl Config {
    pub fn validate(&self) -> Result<(), String> {
        validate_machine(&self.machine)?;
        if self.host.is_empty()
            || self.host.len() > 255
            || self.host.starts_with('-')
            || !self
                .host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-@:".contains(&b))
        {
            return Err(
                "SSH host must be a configured alias or [user@]hostname, without shell characters"
                    .into(),
            );
        }
        // OpenSSH sends a command through the remote account's shell. Keep its entire command grammar
        // fixed and allow only a literal executable path. Shell selection changes invocation syntax,
        // never the allowed characters or the remote program/arguments.
        if self.executable.trim().is_empty()
            || self.executable != self.executable.trim()
            || self.executable.len() > 1024
            || self.executable.starts_with('-')
            || !self
                .executable
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-:/\\ ".contains(&b))
        {
            return Err("remote executable must be agentbolt or a literal local path using letters, digits, spaces, dots, underscores, hyphens and path separators".into());
        }
        if let Some(path) = &self.config_file
            && (!path.is_absolute()
                || !path
                    .to_str()
                    .is_some_and(|s| s.len() <= 4096 && !s.chars().any(char::is_control))
                || !path.is_file())
        {
            return Err("--ssh-config needs an existing absolute configuration file without control characters".into());
        }
        Ok(())
    }
    /// Build the validated, fixed command sent to the configured SSH account's shell.
    pub fn command(&self, channel: &str) -> Result<String, String> {
        self.validate()?;
        Names::for_channel(channel)?;
        // Forward slashes keep Windows paths intact through a POSIX SSH client/remote shell too.
        // PowerShell treats a quoted executable as a string unless invoked with its call operator.
        // sh and cmd use the same command here; all expansion and control characters are refused.
        let invoke = if self.shell == Shell::PowerShell {
            "& "
        } else {
            ""
        };
        Ok(format!(
            "{invoke}\"{}\" relay --channel {channel} --machine {}",
            self.executable.replace('\\', "/"),
            self.machine
        ))
    }
}

pub struct Remote {
    config: Config,
    channel: String,
    idle: Mutex<Vec<Connection>>,
    slots: Semaphore,
    answer_waits: Semaphore,
}
struct Permits<'a> {
    _slot: SemaphorePermit<'a>,
    _wait: Option<SemaphorePermit<'a>>,
}
struct Connection {
    _child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    seq: u64,
}

impl Remote {
    pub fn new(config: Config, channel: String) -> Self {
        Self {
            config,
            channel,
            idle: Mutex::new(Vec::new()),
            slots: Semaphore::new(MAX_CONNECTIONS),
            answer_waits: Semaphore::new(MAX_CONNECTIONS - 1),
        }
    }
    fn permits(&self, op: &McpOp) -> Result<Permits<'_>, CallError> {
        let waiting = matches!(op, McpOp::Answers { wait_ms, .. } | McpOp::AnswersPage { wait_ms, .. } if *wait_ms > 0);
        let wait = if waiting {
            Some(self.answer_waits.try_acquire().map_err(|_| CallError::Refused(
                "Three SSH answer waits are already active. Retry after one finishes, or read with wait_ms: 0.".into()
            ))?)
        } else {
            None
        };
        let slot = self.slots.try_acquire().map_err(|_| {
            CallError::Refused(
                "All four SSH relay connections are busy. Nothing was submitted; retry shortly."
                    .into(),
            )
        })?;
        Ok(Permits {
            _slot: slot,
            _wait: wait,
        })
    }
    async fn connect(&self) -> Result<Connection, CallError> {
        let remote_command = self
            .config
            .command(&self.channel)
            .map_err(CallError::Other)?;
        let mut command = tokio::process::Command::new("ssh");
        if let Some(config_file) = &self.config.config_file {
            command.arg("-F").arg(config_file);
        }
        command
            .args([
                "-T",
                "-o",
                "BatchMode=yes",
                "-o",
                "StrictHostKeyChecking=yes",
                "-o",
                "UpdateHostKeys=no",
                "-o",
                "VerifyHostKeyDNS=no",
                "-o",
                "ClearAllForwardings=yes",
                "-o",
                "ForwardAgent=no",
                "-o",
                "ForwardX11=no",
                "-o",
                "PermitLocalCommand=no",
                "-o",
                "ConnectTimeout=10",
                "-o",
                "ServerAliveInterval=15",
                "-o",
                "ServerAliveCountMax=3",
                "-o",
                "ControlMaster=no",
                "-o",
                "ControlPath=none",
            ])
            .arg(&self.config.host)
            .arg(remote_command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x0800_0000);
        let mut child = command
            .spawn()
            .map_err(|e| CallError::Other(format!("Cannot start SSH: {e}")))?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| CallError::Other("SSH stdin unavailable".into()))?;
        let output = BufReader::new(
            child
                .stdout
                .take()
                .ok_or_else(|| CallError::Other("SSH stdout unavailable".into()))?,
        );
        Ok(Connection {
            _child: child,
            input,
            output,
            seq: 0,
        })
    }
    pub async fn call<T: Serialize + Sync>(&self, op: &T) -> Result<Value, CallError> {
        let value = serde_json::to_value(op).map_err(|e| CallError::Other(e.to_string()))?;
        let typed: McpOp = serde_json::from_value(value).map_err(|_| {
            CallError::Refused(
                "The remote relay accepts only MCP operations; human answers are Desk-only".into(),
            )
        })?;
        if matches!(typed, McpOp::RegisterRemote { .. }) {
            return Err(CallError::Refused(
                "Machine identity belongs to the relay configuration".into(),
            ));
        }
        // Bound active children, not just the idle pool. One slot is reserved from long answer waits
        // so a host waiting for decisions can still ask, renew work and read its Board.
        let _permits = self.permits(&typed)?;
        // Only safe reads are retried automatically. A disconnected write may already have committed.
        let retry = retryable(&typed);
        for attempt in 0..2 {
            let pooled = self.idle.lock().unwrap_or_else(|e| e.into_inner()).pop();
            let mut connection = match pooled {
                Some(c) => c,
                None => self.connect().await?,
            };
            let result = connection.call(typed.clone()).await;
            if matches!(result, Ok(_) | Err(CallError::Refused(_))) {
                let mut idle = self.idle.lock().unwrap_or_else(|e| e.into_inner());
                if idle.len() < MAX_CONNECTIONS {
                    idle.push(connection);
                }
                return result;
            }
            if !retry || attempt == 1 {
                return result;
            }
        }
        unreachable!()
    }
}

pub(crate) fn retryable(op: &McpOp) -> bool {
    matches!(
        op,
        McpOp::Status
            | McpOp::Answers { .. }
            | McpOp::AnswersPage { .. }
            | McpOp::DeliveryHistory { .. }
            | McpOp::RelayList { .. }
            | McpOp::RelayPage { .. }
    ) || matches!(
        op,
        McpOp::Board {
            action: agentbolt_core::board::BoardOp::Who
                | agentbolt_core::board::BoardOp::WhoPage { .. }
                | agentbolt_core::board::BoardOp::Read { .. }
                | agentbolt_core::board::BoardOp::Requests { .. }
                | agentbolt_core::board::BoardOp::Get { .. },
            ..
        } | McpOp::Holds {
            action: agentbolt_core::holds::HoldOp::List
                | agentbolt_core::holds::HoldOp::ListPage { .. },
            ..
        }
    )
}

impl Connection {
    async fn call(&mut self, op: McpOp) -> Result<Value, CallError> {
        self.seq = self
            .seq
            .checked_add(1)
            .ok_or_else(|| CallError::Other("SSH sequence exhausted".into()))?;
        let request = Request { id: self.seq, op };
        let mut bytes =
            serde_json::to_vec(&request).map_err(|e| CallError::Other(e.to_string()))?;
        if bytes.len() > MAX_REQUEST {
            return Err(CallError::Refused("relay request exceeds 1 MiB".into()));
        }
        bytes.push(b'\n');
        let response=tokio::time::timeout(Duration::from_secs(630),async {
            self.input.write_all(&bytes).await.map_err(|e|e.to_string())?;
            self.input.flush().await.map_err(|e|e.to_string())?;
            let line=line(&mut self.output,MAX_RESPONSE).await?.ok_or("SSH connection ended")?;
            let response:Response=serde_json::from_slice(&line).map_err(|_|"SSH returned something other than an Agentbolt response")?;
            if response.id!=self.seq{return Err("SSH response sequence mismatch".into());}
            Ok::<_,String>(response)
        }).await.map_err(|_|CallError::Other("SSH relay timed out; the operation may have committed. Reconnect and read its durable state before retrying a write.".into()))?
            .map_err(|e|CallError::Other(format!("SSH relay failed: {e}. Verify key authentication, the known host, the installed Agentbolt path and --remote-shell. A submitted operation may have committed; read its state before retrying.")))?;
        if response.ok {
            response
                .result
                .ok_or_else(|| CallError::Other("SSH relay returned no result".into()))
        } else {
            Err(CallError::Refused(
                response
                    .error
                    .unwrap_or_else(|| "remote operation refused".into()),
            ))
        }
    }
}

async fn line(
    reader: &mut (impl AsyncBufRead + Unpin),
    limit: usize,
) -> Result<Option<Vec<u8>>, String> {
    let mut result = Vec::new();
    loop {
        let available = reader.fill_buf().await.map_err(|e| e.to_string())?;
        if available.is_empty() {
            return if result.is_empty() {
                Ok(None)
            } else {
                Err("incomplete relay line".into())
            };
        }
        let end = available.iter().position(|b| *b == b'\n');
        let take = end.map_or(available.len(), |n| n + 1);
        if result.len() + take > limit + 1 {
            return Err("relay line exceeds its size limit".into());
        }
        result.extend_from_slice(&available[..take]);
        reader.consume(take);
        if end.is_some() {
            result.pop();
            return Ok(Some(result));
        }
    }
}

/// Run on the machine hosting the Desk, under its authenticated user's SSH account. The byte stream is
/// typed as McpOp before forwarding; DeskOp and caller-chosen remote identities cannot cross this bridge.
pub async fn relay(names: Names, machine: &str) -> anyhow::Result<()> {
    validate_machine(machine).map_err(anyhow::Error::msg)?;
    let daemon = Daemon::new(names, Pipe::Mcp, true);
    let mut input = BufReader::new(tokio::io::stdin());
    let mut output = tokio::io::stdout();
    while let Some(bytes) = line(&mut input, MAX_REQUEST)
        .await
        .map_err(anyhow::Error::msg)?
    {
        let request: Request<McpOp> = serde_json::from_slice(&bytes)?;
        let result = match request.op {
            McpOp::Register {
                owner,
                client,
                session_ref,
                cwd,
                doing,
                provisional_key,
            } => {
                daemon
                    .call(&McpOp::RegisterRemote {
                        machine: machine.into(),
                        registration: RemoteRegistration {
                            owner,
                            client,
                            session_ref,
                            cwd,
                            doing,
                            provisional_key,
                        },
                    })
                    .await
            }
            McpOp::RegisterRemote { .. } => Err(CallError::Refused(
                "Machine identity belongs to relay configuration".into(),
            )),
            op => daemon.call(&op).await,
        };
        let response = match result {
            Ok(result) => Response {
                id: request.id,
                ok: true,
                result: Some(result),
                error: None,
            },
            Err(e) => Response {
                id: request.id,
                ok: false,
                result: None,
                error: Some(e.to_string()),
            },
        };
        let mut encoded = serde_json::to_vec(&response)?;
        if encoded.len() > MAX_RESPONSE {
            anyhow::bail!("relay response exceeds 16 MiB");
        }
        encoded.push(b'\n');
        output.write_all(&encoded).await?;
        output.flush().await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentbolt_core::{board::BoardOp, holds::HoldOp};

    #[test]
    fn explicit_config_is_a_local_existing_file_and_not_part_of_remote_command() {
        let temp = crate::testutil::TempDir::new();
        let path = temp.path().join("host config with spaces");
        std::fs::write(&path, "Host fixture\n  HostName 127.0.0.1\n").unwrap();
        let mut config = Config {
            host: "fixture".into(),
            machine: "test-client".into(),
            executable: "agentbolt".into(),
            shell: Shell::Sh,
            config_file: Some(path),
        };
        assert_eq!(
            config.command("remote-test").unwrap(),
            "\"agentbolt\" relay --channel remote-test --machine test-client"
        );
        for invalid in [
            PathBuf::from("relative-file"),
            temp.path().to_owned(),
            temp.path().join("missing"),
            temp.path().join("bad\nfile"),
        ] {
            config.config_file = Some(invalid);
            assert!(config.validate().is_err());
        }
    }

    #[test]
    fn ssh_command_has_no_shell_expansion_or_caller_selected_identity() {
        let base = Config {
            host: "artem@desk.example.test".into(),
            machine: "laptop-2".into(),
            executable: "C:\\Program Files\\Agentbolt\\agentbolt.exe".into(),
            shell: Shell::Sh,
            config_file: None,
        };
        assert_eq!(
            base.command("remote-test").unwrap(),
            "\"C:/Program Files/Agentbolt/agentbolt.exe\" relay --channel remote-test --machine laptop-2"
        );
        let mut powershell = base.clone();
        powershell.shell = Shell::PowerShell;
        assert_eq!(
            powershell.command("remote-test").unwrap(),
            "& \"C:/Program Files/Agentbolt/agentbolt.exe\" relay --channel remote-test --machine laptop-2"
        );
        let mut cmd = base.clone();
        cmd.shell = Shell::Cmd;
        assert_eq!(
            cmd.command("remote-test").unwrap(),
            base.command("remote-test").unwrap()
        );
        for host in [
            "-oProxyCommand=bad",
            "desk;bad",
            "desk\ncommand",
            "$(command)",
            "desk`command`",
            "desk name",
        ] {
            let mut cfg = base.clone();
            cfg.host = host.into();
            assert!(cfg.command("remote-test").is_err(), "{host}");
        }
        for executable in [
            "agentbolt;bad",
            "$(command)",
            "~/agentbolt",
            "%TEMP%/agentbolt",
            "x\" y",
            "-x",
            " agentbolt",
            "agentbolt\n",
            "agentbolt`command`",
            "agentbolt&command",
            "agentbolt|command",
            "agentbolt'command",
        ] {
            for shell in [Shell::Sh, Shell::Cmd, Shell::PowerShell] {
                let mut cfg = base.clone();
                cfg.shell = shell;
                cfg.executable = executable.into();
                assert!(
                    cfg.command("remote-test").is_err(),
                    "{shell:?}: {executable}"
                );
            }
        }
        for machine in ["", "two machines", "desk;command", "desk\ncommand"] {
            let mut cfg = base.clone();
            cfg.machine = machine.into();
            assert!(cfg.command("remote-test").is_err(), "{machine}");
        }
        assert!(base.command("main;bad").is_err());
    }

    #[tokio::test]
    async fn remote_refuses_human_answers_and_forged_registration_before_connecting() {
        let remote = Remote::new(
            Config {
                host: "unused.invalid".into(),
                machine: "test".into(),
                executable: "agentbolt".into(),
                shell: Shell::Sh,
                config_file: None,
            },
            "remote-test".into(),
        );
        for value in [
            serde_json::json!({"op":"answer","ask_id":"ask_test","choice":"Yes"}),
            serde_json::json!({"op":"phone_reply","id":"receipt"}),
        ] {
            assert!(matches!(
                remote.call(&value).await,
                Err(CallError::Refused(_))
            ));
        }
        let forged = McpOp::RegisterRemote {
            machine: "local".into(),
            registration: RemoteRegistration {
                owner: None,
                client: "fake".into(),
                session_ref: None,
                cwd: None,
                doing: None,
                provisional_key: None,
            },
        };
        assert!(matches!(
            remote.call(&forged).await,
            Err(CallError::Refused(_))
        ));
    }

    #[test]
    fn only_reads_are_replayed_after_an_uncertain_disconnect() {
        let wrap = |action| McpOp::Board {
            handle: "handle".into(),
            action,
        };
        assert!(retryable(&McpOp::Status));
        assert!(retryable(&wrap(BoardOp::Who)));
        assert!(!retryable(&wrap(BoardOp::Claim {
            id: "request".into()
        })));
        assert!(!retryable(&wrap(BoardOp::Done {
            id: "request".into(),
            fence: 1,
            result: "Done".into(),
            failed: false
        })));
        assert!(!retryable(&McpOp::Cancel {
            handle: "handle".into(),
            id: "ask".into()
        }));
        assert!(!retryable(&McpOp::Holds {
            handle: "handle".into(),
            action: HoldOp::Check {
                repo: "example/project".into(),
                paths: vec!["src/**".into()],
                stage: Some("merge".into())
            }
        }));
    }

    #[tokio::test]
    async fn relay_reader_bounds_fragmented_input_and_keeps_next_message() {
        let mut reader = BufReader::with_capacity(2, &b"1234\nnext\n"[..]);
        assert_eq!(line(&mut reader, 4).await.unwrap(), Some(b"1234".to_vec()));
        assert_eq!(line(&mut reader, 4).await.unwrap(), Some(b"next".to_vec()));
        assert_eq!(line(&mut reader, 4).await.unwrap(), None);
        let mut oversized = BufReader::with_capacity(2, &b"12345\n"[..]);
        assert!(line(&mut oversized, 4).await.unwrap_err().contains("limit"));
        let mut truncated = BufReader::with_capacity(2, &b"123"[..]);
        assert!(
            line(&mut truncated, 4)
                .await
                .unwrap_err()
                .contains("incomplete")
        );
    }

    #[tokio::test]
    async fn active_connection_limit_reserves_room_for_work_and_releases_on_cancellation() {
        let remote = Remote::new(
            Config {
                host: "unused.invalid".into(),
                machine: "test".into(),
                executable: "agentbolt".into(),
                shell: Shell::Sh,
                config_file: None,
            },
            "remote-test".into(),
        );
        let waiting = McpOp::AnswersPage {
            handle: "handle".into(),
            before: None,
            wait_ms: 600_000,
        };
        let wait1 = remote.permits(&waiting).unwrap();
        let wait2 = remote.permits(&waiting).unwrap();
        let wait3 = remote.permits(&waiting).unwrap();
        // Refusal happens before SSH is started, so this test cannot contact the placeholder host.
        assert!(matches!(
            remote.call(&waiting).await,
            Err(CallError::Refused(_))
        ));
        let work = remote.permits(&McpOp::Status).unwrap();
        assert!(matches!(
            remote.call(&McpOp::Status).await,
            Err(CallError::Refused(_))
        ));
        drop(wait1);
        let replacement = remote.permits(&waiting).unwrap();
        drop((wait2, wait3, work, replacement));
        let permits = (0..MAX_CONNECTIONS)
            .map(|_| remote.permits(&McpOp::Status).unwrap())
            .collect::<Vec<_>>();
        assert!(remote.permits(&McpOp::Status).is_err());
        drop(permits);
        assert!(remote.permits(&waiting).is_ok());
    }
}
