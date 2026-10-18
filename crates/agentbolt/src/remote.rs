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
use std::{process::Stdio, sync::Mutex, time::Duration};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout},
};

const MAX_REQUEST: usize = 1 << 20;
const MAX_RESPONSE: usize = 16 << 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub host: String,
    pub machine: String,
    pub executable: String,
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
        // fixed and allow only a literal executable path, usable by both cmd.exe and POSIX shells.
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
        Ok(())
    }
    fn command(&self, channel: &str) -> Result<String, String> {
        self.validate()?;
        Names::for_channel(channel)?;
        // Forward slashes keep Windows paths intact through a POSIX SSH client/remote shell too.
        Ok(format!(
            "\"{}\" relay --channel {channel} --machine {}",
            self.executable.replace('\\', "/"),
            self.machine
        ))
    }
}

pub struct Remote {
    config: Config,
    channel: String,
    idle: Mutex<Vec<Connection>>,
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
        }
    }
    async fn connect(&self) -> Result<Connection, CallError> {
        let remote_command = self
            .config
            .command(&self.channel)
            .map_err(CallError::Other)?;
        let mut command = tokio::process::Command::new("ssh");
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
                if idle.len() < 4 {
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
            .map_err(|e|CallError::Other(format!("SSH relay failed: {e}. Verify key authentication, the known host and the installed Agentbolt path. A submitted operation may have committed; read its state before retrying.")))?;
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
