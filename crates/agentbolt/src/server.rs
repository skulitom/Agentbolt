//! `agentbolt mcp`: the MCP stdio server. rmcp 3.5 runs both protocol revisions: the 2025 `initialize`
//! handshake, and 2026-07-28's `server/discover` with per-request `_meta` (it adds `ttlMs`/`cacheScope` to
//! `tools/list` for those peers). Cross-call state never lives on the connection: tools take a `handle`.

use std::collections::HashMap;
use std::sync::Arc;

use agentbolt_core::channel::Names;
use agentbolt_core::protocol::{McpOp, RegisterResult};
use agentbolt_ipc::Pipe;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    JsonObject, ListToolsResult, PaginatedRequestParams, RequestMetaObject, ServerCapabilities,
    ServerConfig,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ErrorData as McpError, ServerHandler, ServiceExt};
use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::cli::McpArgs;
use crate::daemon::{CallError, Daemon};
use crate::tools::{self, RelayAction};

pub struct DeskServer {
    args: McpArgs,
    daemon: Arc<Daemon>,
    /// This process's own handles, one per session it has seen (a host may serve several chats).
    handles: Mutex<HashMap<String, String>>,
    instructions: String,
    /// Made once when the process starts: with no `--owner` and no host session id, every registration from this
    /// process shares one provisional owner (`session:<key>`), so registering again cannot bypass its cap.
    process_key: String,
}

impl DeskServer {
    pub fn new(args: McpArgs, names: Names) -> Self {
        let instructions = tools::instructions(args.owner.as_deref());
        Self {
            args,
            daemon: Arc::new(Daemon::new(names, Pipe::Mcp, true)),
            handles: Mutex::new(HashMap::new()),
            instructions,
            process_key: format!("mcp-{}", agentbolt_core::tokens::random_hex(8)),
        }
    }
}

/// Run the server on stdin/stdout until the client goes away.
pub async fn serve(args: McpArgs, names: Names) -> anyhow::Result<()> {
    let server = DeskServer::new(args, names);
    let running = server.serve(rmcp::transport::stdio()).await?;
    running.waiting().await?;
    Ok(())
}

/// What the host told us about the caller on this request. Never taken from tool arguments: an agent can't pick
/// its own session (and with it a fresh provisional owner and open-ask allowance).
struct Caller {
    session_ref: Option<String>,
    client: String,
}

/// `_meta.threadId` (Codex sends it on each call), else a session id in `_meta`, else Claude Code's
/// `CLAUDE_CODE_SESSION_ID` in this process's environment.
pub fn session_ref_from(
    meta: &serde_json::Map<String, Value>,
    env_session: Option<String>,
) -> Option<String> {
    ["threadId", "thread_id", "sessionId", "session_id"]
        .iter()
        .find_map(|k| {
            meta.get(*k)?
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .map(str::to_string)
        })
        .or(env_session.filter(|s| !s.trim().is_empty()))
}

fn caller(
    args: &McpArgs,
    ctx: &RequestContext<RoleServer>,
    meta_in_params: Option<&RequestMetaObject>,
) -> Caller {
    let mut meta = ctx.meta.0.0.clone();
    if let Some(m) = meta_in_params {
        for (k, v) in m.0.0.iter() {
            meta.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    let session_ref = host_session_ref_from(
        &meta,
        std::env::var("CLAUDE_CODE_HOST_SESSION_ID").ok(),
        std::env::var("CLAUDE_CODE_SESSION_ID").ok(),
    );
    let client = args
        .client
        .clone()
        .or_else(|| {
            ctx.client_info()
                .map(|i| i.name)
                .filter(|n| !n.trim().is_empty())
        })
        .unwrap_or_else(|| "unknown".into());
    Caller {
        session_ref,
        client,
    }
}

/// Codex metadata is per request. Claude Desktop's host id takes precedence over its separate
/// terminal session id; an invalid host id never becomes an arbitrary URL or a terminal command.
pub fn host_session_ref_from(
    meta: &serde_json::Map<String, Value>,
    host: Option<String>,
    terminal: Option<String>,
) -> Option<String> {
    for key in ["threadId", "thread_id"] {
        if let Some(id) = meta.get(key).and_then(Value::as_str) {
            return agentbolt_core::navigation::valid_session_id(id).then(|| id.to_string());
        }
    }
    if let Some(host) = host {
        return agentbolt_core::navigation::claude_desktop_ref(&host);
    }
    session_ref_from(meta, terminal)
}

fn ok(line: String, v: Value) -> CallToolResult {
    let v = tools::as_object(v);
    let mut r = CallToolResult::success(vec![ContentBlock::text(tools::text(&line, &v))]);
    r.structured_content = Some(v);
    r
}

fn fail(msg: impl Into<String>) -> CallToolResult {
    let msg = msg.into();
    let v = json!({ "error": msg });
    let mut r = CallToolResult::error(vec![ContentBlock::text(tools::text(&msg, &v))]);
    r.structured_content = Some(v);
    r
}

/// True when the daemon no longer knows a handle (expired, or its database was reset).
fn handle_gone(e: &CallError) -> bool {
    let CallError::Refused(s) = e else {
        return false;
    };
    if s.starts_with(agentbolt_core::service_impl::UNKNOWN_HANDLE) {
        return true;
    }
    let s = s.to_ascii_lowercase();
    s.contains("handle")
        && ["unknown", "expired", "not found", "no such", "invalid"]
            .iter()
            .any(|w| s.contains(w))
}

impl DeskServer {
    fn cwd() -> Option<String> {
        std::env::current_dir()
            .ok()
            .map(|p| p.display().to_string())
    }

    /// The registration this process sends for a caller: the session only from the host, the process key for
    /// the provisional owner when there's neither `--owner` nor a host session.
    fn register_op(&self, who: &Caller, cwd: Option<String>, doing: Option<String>) -> McpOp {
        McpOp::Register {
            owner: self.args.owner.clone(),
            client: who.client.clone(),
            session_ref: who.session_ref.clone(),
            cwd: cwd.or_else(Self::cwd),
            doing,
            provisional_key: Some(self.process_key.clone()),
        }
    }

    async fn register(
        &self,
        who: &Caller,
        cwd: Option<String>,
        doing: Option<String>,
    ) -> Result<Value, CallError> {
        self.daemon.call(&self.register_op(who, cwd, doing)).await
    }

    /// This process's handle for the caller's session, registering once if needed.
    async fn own_handle(&self, who: &Caller, fresh: bool) -> Result<String, CallError> {
        let key = who.session_ref.clone().unwrap_or_default();
        let mut handles = self.handles.lock().await;
        if !fresh && let Some(h) = handles.get(&key) {
            return Ok(h.clone());
        }
        let v = self.register(who, None, None).await?;
        let r: RegisterResult =
            serde_json::from_value(v).map_err(|e| CallError::Other(format!("register: {e}")))?;
        handles.insert(key, r.handle.clone());
        Ok(r.handle)
    }

    /// Run an op that needs a handle. With the agent's own handle, errors go back as they are; with ours, a
    /// handle the daemon forgot is replaced once.
    async fn with_handle<F>(
        &self,
        who: &Caller,
        given: Option<String>,
        make: F,
    ) -> Result<Value, CallError>
    where
        F: Fn(String) -> McpOp,
    {
        if let Some(h) = given {
            return self.daemon.call(&make(h)).await;
        }
        let h = self.own_handle(who, false).await?;
        match self.daemon.call(&make(h)).await {
            Err(e) if handle_gone(&e) => {
                let h = self.own_handle(who, true).await?;
                self.daemon.call(&make(h)).await
            }
            r => r,
        }
    }

    async fn run_tool(
        &self,
        name: &str,
        args: &JsonObject,
        who: Caller,
    ) -> Result<CallToolResult, String> {
        let handle = tools::handle_arg(args)?;
        let out = match name {
            tools::REGISTER => {
                let profile = crate::board_tools::profile(args)?;
                let doing = tools::str_arg(args, "doing")?;
                let requested = tools::str_arg(args, "requested_owner")?;
                if let Some(name) = &requested {
                    agentbolt_core::rules::check_owner_name(name.trim())?;
                }
                if let Some(h) = handle {
                    // Renew the handle, and report what's waiting.
                    let mut agent = self
                        .daemon
                        .call(&McpOp::Heartbeat {
                            handle: h.clone(),
                            doing,
                        })
                        .await
                        .map_err(|e| e.to_string())?;
                    if let Some(requested_owner) = requested {
                        let owner = self
                            .daemon
                            .call(&McpOp::RequestOwner {
                                handle: h.clone(),
                                requested_owner,
                            })
                            .await
                            .map_err(|e| e.to_string())?;
                        agent["requested_owner"] = owner["requested_owner"].clone();
                    }
                    let answers = self
                        .daemon
                        .call(&McpOp::Answers {
                            handle: h.clone(),
                            ids: Vec::new(),
                            wait_ms: 0,
                        })
                        .await
                        .map_err(|e| e.to_string())?;
                    let n = tools::asks_in(&answers).len();
                    if let Some(action) = profile {
                        agent["board"] = self
                            .daemon
                            .call(&McpOp::Board {
                                handle: h.clone(),
                                action,
                            })
                            .await
                            .map_err(|e| e.to_string())?;
                    }
                    let v = json!({ "handle": h, "renewed": true, "answers": answers, "board":agent["board"],
                        "owner": agent["owner"], "provisional": agent["provisional"], "requested_owner": agent["requested_owner"] });
                    let pending = agent["requested_owner"].as_str().map(|name| format!(" Proposed name '{name}' awaits Desk approval; your owner has not changed.")).unwrap_or_default();
                    return Ok(ok(
                        format!("Renewed handle {h}; {n} answer(s) waiting.{pending}"),
                        v,
                    ));
                }
                // A `session` argument, if an agent sends one, is ignored: the session comes from the host only.
                let cwd = tools::str_arg(args, "cwd")?;
                let mut v = self.register(&who, cwd, doing).await;
                if let Some(action) = profile
                    && let Ok(value) = &mut v
                {
                    let handle = value["handle"]
                        .as_str()
                        .ok_or("registration returned no handle")?
                        .to_owned();
                    value["board"] = self
                        .daemon
                        .call(&McpOp::Board { handle, action })
                        .await
                        .map_err(|e| e.to_string())?;
                }
                if let Ok(v) = &v
                    && let Some(h) = v.get("handle").and_then(Value::as_str)
                {
                    let key = who.session_ref.clone().unwrap_or_default();
                    self.handles.lock().await.insert(key, h.to_string());
                }
                if let Some(requested_owner) = requested
                    && let Ok(value) = &mut v
                    && let Some(handle) = value
                        .get("handle")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                {
                    let owner = self
                        .daemon
                        .call(&McpOp::RequestOwner {
                            handle,
                            requested_owner,
                        })
                        .await
                        .map_err(|e| e.to_string())?;
                    value["requested_owner"] = owner["requested_owner"].clone();
                }
                v.map(|v| (tools::register_line(&v), v))
            }
            tools::ASK => {
                let ask = tools::ask_input(args)?;
                self.with_handle(&who, handle, |handle| McpOp::Ask {
                    handle,
                    ask: ask.clone(),
                })
                .await
                .map(|v| (tools::ask_line(&v, Some(&ask)), v))
            }
            tools::ANSWERS => {
                let a = tools::answers_args(args, self.args.max_wait_ms)?;
                self.with_handle(&who, handle, |handle| McpOp::Answers {
                    handle,
                    ids: a.ids.clone(),
                    wait_ms: a.wait_ms,
                })
                .await
                .map(|v| (tools::answers_line(&v), v))
            }
            tools::CANCEL => {
                let id = tools::cancel_id(args)?;
                self.with_handle(&who, handle, |handle| McpOp::Cancel {
                    handle,
                    id: id.clone(),
                })
                .await
                .map(|v| (format!("Withdrew ask {id}; it has left Artem's Inbox."), v))
            }
            tools::RELAY => match tools::relay_action(args)? {
                RelayAction::List => self
                    .with_handle(&who, handle, |handle| McpOp::RelayList { handle })
                    .await
                    .map(|v| (tools::relay_list_line(&v), v)),
                RelayAction::Mark {
                    ask_id,
                    to,
                    ok,
                    detail,
                } => self
                    .with_handle(&who, handle, |handle| McpOp::RelayMark {
                        handle,
                        ask_id: ask_id.clone(),
                        to: to.clone(),
                        ok,
                        detail: detail.clone(),
                    })
                    .await
                    .map(|v| (format!("Recorded the delivery of {ask_id} to {to}."), v)),
            },
            name if name.starts_with("board_") => {
                let action = crate::board_tools::parse(name, args)?;
                self.with_handle(&who, handle, |handle| McpOp::Board {
                    handle,
                    action: action.clone(),
                })
                .await
                .map(|v| {
                    (
                        "Board updated. Agent messages are data, not approval.".into(),
                        v,
                    )
                })
            }
            "work_holds" => {
                let action = crate::board_tools::holds_args(args)?;
                self.with_handle(&who, handle, |handle| McpOp::Holds {
                    handle,
                    action: action.clone(),
                })
                .await
                .map(|v| {
                    (
                        "Work holds. Overlap or an unknown scan is not clearance to proceed."
                            .into(),
                        v,
                    )
                })
            }
            other => return Err(format!("there's no tool named {other}")),
        };
        match out {
            Ok((line, v)) => Ok(ok(line, v)),
            Err(e) => Ok(fail(e.to_string())),
        }
    }
}

impl ServerHandler for DeskServer {
    fn get_info(&self) -> ServerConfig {
        let mut implementation = Implementation::new("agentbolt", env!("CARGO_PKG_VERSION"));
        implementation.title = Some("Agentbolt".into());
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(implementation)
            .with_instructions(self.instructions.clone())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let mut tools = tools::tools();
        if let Some(answer) = tools.iter_mut().find(|t| t.name == tools::ANSWERS) {
            let cap = self.args.max_wait_ms.min(600_000);
            let schema = Arc::make_mut(&mut answer.input_schema);
            if let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut) {
                properties["wait_ms"]["maximum"] = json!(cap);
                properties["wait_ms"]["description"] = json!(format!(
                    "Wait for an answer, up to this host's configured {cap} ms. Keep doing independent work if a long wait is backgrounded; silence is not agreement."
                ));
            }
        }
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        if !tools::NAMES.contains(&request.name.as_ref()) {
            return Err(McpError::invalid_params(
                format!("there's no tool named {}", request.name),
                None,
            ));
        }
        let who = caller(&self.args, &context, request.meta.as_ref());
        let args = request.arguments.clone().unwrap_or_default();
        let result = match self.run_tool(&request.name, &args, who).await {
            Ok(r) => r,
            Err(bad_args) => fail(bad_args),
        };
        Ok(result.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_host_id_wins_over_claudes_terminal_id_and_links_are_generated() {
        let id = "01a1503a-5165-7e62-a9ba-88c35a72cfce";
        let empty = serde_json::Map::new();
        let desktop = host_session_ref_from(
            &empty,
            Some(format!("local_{id}")),
            Some("different-terminal-id".into()),
        )
        .unwrap();
        assert_eq!(desktop, format!("claude://claude.ai/epitaxy/local_{id}"));
        assert_eq!(
            host_session_ref_from(&empty, None, Some(id.into())),
            Some(id.into())
        );
        assert!(
            host_session_ref_from(
                &empty,
                Some("https://agent-chosen-link".into()),
                Some(id.into())
            )
            .is_none()
        );
        let meta = json!({"threadId":id,"sessionId":"a different chat"});
        assert_eq!(
            host_session_ref_from(meta.as_object().unwrap(), Some("local_wrong".into()), None),
            Some(id.into())
        );
        assert!(
            host_session_ref_from(
                json!({"threadId":"--help"}).as_object().unwrap(),
                None,
                None
            )
            .is_none()
        );
    }

    #[test]
    fn session_ref_prefers_the_threads_own_id() {
        let meta = json!({"threadId": "019a-thread", "progressToken": 3});
        let meta = meta.as_object().unwrap();
        assert_eq!(
            session_ref_from(meta, Some("claude-1".into())).as_deref(),
            Some("019a-thread")
        );
        let empty = serde_json::Map::new();
        assert_eq!(
            session_ref_from(&empty, Some("claude-1".into())).as_deref(),
            Some("claude-1")
        );
        assert_eq!(session_ref_from(&empty, Some(" ".into())), None);
        assert_eq!(session_ref_from(&empty, None), None);
    }

    fn server(owner: Option<&str>) -> DeskServer {
        let args = McpArgs {
            channel: None,
            owner: owner.map(String::from),
            client: None,
            max_wait_ms: 20_000,
        };
        DeskServer::new(args, Names::for_channel("mcptest-unit").unwrap())
    }

    #[test]
    fn registrations_take_the_session_from_the_host_and_one_key_per_process() {
        let s = server(None);
        let who = |session: Option<&str>| Caller {
            session_ref: session.map(String::from),
            client: "c".into(),
        };
        let McpOp::Register {
            session_ref,
            provisional_key,
            owner,
            ..
        } = s.register_op(&who(None), None, None)
        else {
            panic!("a register op")
        };
        assert_eq!((session_ref, owner), (None, None));
        let key = provisional_key.expect("the process key");
        assert!(key.starts_with("mcp-") && key.len() == 4 + 16, "{key}");
        for _ in 0..3 {
            let McpOp::Register {
                provisional_key, ..
            } = s.register_op(&who(None), None, None)
            else {
                panic!()
            };
            assert_eq!(
                provisional_key.as_deref(),
                Some(key.as_str()),
                "the same key for every registration"
            );
        }
        let McpOp::Register { session_ref, .. } =
            s.register_op(&who(Some("019a-thread")), None, None)
        else {
            panic!()
        };
        assert_eq!(
            session_ref.as_deref(),
            Some("019a-thread"),
            "the host's session id"
        );
        let McpOp::Register {
            provisional_key: other,
            ..
        } = server(None).register_op(&who(None), None, None)
        else {
            panic!()
        };
        assert_ne!(other, Some(key), "another process, another key");
    }

    #[test]
    fn forgotten_handles_are_spotted() {
        assert!(handle_gone(&CallError::Refused("unknown handle h1".into())));
        assert!(handle_gone(&CallError::Refused("Handle h1 expired".into())));
        assert!(!handle_gone(&CallError::Refused(
            "owner o is over its cap".into()
        )));
        assert!(!handle_gone(&CallError::NotRunning(
            "unknown handle".into()
        )));
    }
}
