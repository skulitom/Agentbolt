//! Protected ntfy questions/replies. The network worker queues receipts, then invokes the installed
//! Desk with an opaque receipt id. It never calls an answer operation or puts tokens on command lines.
use crate::daemon::Shared;
use agentbolt_core::phone::{
    self, Config, Incoming, Notification, ResultValue, Secret, State, Work,
};
use serde_json::{Value, json};
use std::{process::Stdio, sync::Arc, time::Duration};

#[cfg(test)]
#[path = "phone_tests.rs"]
mod tests;

async fn work(shared: &Arc<Shared>, generation: &str, op: Work) -> Result<ResultValue, String> {
    let svc = shared.svc.clone();
    let generation = generation.to_owned();
    tokio::task::spawn_blocking(move || svc.phone_work(&generation, op))
        .await
        .map_err(|_| "phone worker unavailable".to_string())?
}

pub async fn run(shared: Arc<Shared>) {
    let mut failures = 0u32;
    loop {
        let state = work(&shared, "", Work::State).await;
        let delay = if let Ok(ResultValue::State(state)) = state {
            if state.config.settings.enabled {
                let generation = state.config.generation.clone();
                match cycle(&shared, state).await {
                    Ok(()) => {
                        failures = 0;
                        15
                    }
                    Err(error) => {
                        let _ = work(&shared, &generation, Work::Fault(error)).await;
                        failures = failures.saturating_add(1);
                        (15u64 * (1u64 << failures.min(5))).min(300)
                    }
                }
            } else {
                failures = 0;
                15
            }
        } else {
            30
        };
        tokio::time::sleep(Duration::from_secs(delay)).await;
    }
}

async fn cycle(shared: &Arc<Shared>, state: State) -> Result<(), String> {
    let cfg = state.config;
    // Receipts are already authenticated and durable. Finishing their local Desk activation must not
    // depend on another successful network round trip. The Desk still checks expiry/revocation.
    if let ResultValue::Pending { ids, .. } = work(shared, &cfg.generation, Work::Pending).await? {
        for id in ids {
            deliver(shared, &id).await?;
        }
    }
    let now = agentbolt_core::store::to_ms(time::OffsetDateTime::now_utc());
    if !state
        .health
        .protected_at
        .is_some_and(|at| now.saturating_sub(at) < 60_000)
    {
        let config = cfg.clone();
        tokio::task::spawn_blocking(move || protection(&config))
            .await
            .map_err(|_| "phone protection check interrupted".to_string())??;
        work(shared, &cfg.generation, Work::Verified).await?;
    }
    let config = cfg.clone();
    let (messages, cursor) =
        tokio::task::spawn_blocking(move || poll(&config, state.cursor.as_deref()))
            .await
            .map_err(|_| "phone poll interrupted".to_string())??;
    work(shared, &cfg.generation, Work::Receive { messages, cursor }).await?;
    if let ResultValue::Pending {
        ids,
        acknowledgements,
    } = work(shared, &cfg.generation, Work::Pending).await?
    {
        for id in ids {
            deliver(shared, &id).await?;
        }
        for ack in acknowledgements {
            let config = cfg.clone();
            let id = ack.id.clone();
            tokio::task::spawn_blocking(move || {
                publish(
                    &config,
                    json!({
                        "topic":config.settings.questions,"title":"Agentbolt reply","priority":2,
                        "message":format!("{}\nQuestion: {}",ack.outcome,ack.ask_id),
                        "sequence_id":format!("reply-{}",ack.id)
                    }),
                )
            })
            .await
            .map_err(|_| "phone acknowledgement interrupted".to_string())??;
            work(shared, &cfg.generation, Work::Acknowledged(id)).await?;
        }
    }
    if let ResultValue::Notification(Some(notification)) =
        work(shared, &cfg.generation, Work::Prepare).await?
    {
        let config = cfg.clone();
        let toast = notification.toast_id.clone();
        tokio::task::spawn_blocking(move || publish_question(&config, notification))
            .await
            .map_err(|_| "phone publication interrupted".to_string())??;
        work(shared, &cfg.generation, Work::Published(toast)).await?;
    }
    Ok(())
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into()
}

/// No anonymous content reads and no test publications. Both topics must reject an anonymous auth
/// check, and the configured account must pass the same check. Rechecked once a minute; never use
/// a guessed topic name as access control. Administrators must also deny anonymous publishing.
fn protection(cfg: &Config) -> Result<(), String> {
    let agent = agent();
    for topic in [&cfg.settings.questions, &cfg.settings.replies] {
        let url = format!("{}/{topic}/auth", cfg.settings.server);
        let anonymous = agent
            .get(&url)
            .call()
            .map_err(|_| "could not check anonymous access to the phone topics".to_string())?;
        if !matches!(anonymous.status().as_u16(), 401 | 403) {
            return Err("phone topics must reject anonymous readers; use private topics with access controls".into());
        }
        let mut authenticated = agent
            .get(&url)
            .header("Authorization", &format!("Bearer {}", cfg.daemon_token.0))
            .call()
            .map_err(|_| "could not authenticate to the phone topics".to_string())?;
        if !authenticated.status().is_success() {
            return Err("ntfy refused the account token; it needs access to both topics".into());
        }
        let body = authenticated
            .body_mut()
            .with_config()
            .limit(4096)
            .read_to_string()
            .map_err(|_| "invalid ntfy authentication response".to_string())?;
        let body: Value = serde_json::from_str(&body)
            .map_err(|_| "invalid ntfy authentication response".to_string())?;
        if body["success"] != true {
            return Err("the server did not confirm ntfy authentication".into());
        }
    }
    Ok(())
}

fn publish_question(cfg: &Config, notification: Notification) -> Result<(), String> {
    let actions:Vec<_>=notification.buttons.into_iter().map(|(label,token)|json!({
        "action":"http","label":label,"url":format!("{}/{}",cfg.settings.server,cfg.settings.replies),
        "method":"POST","headers":{"Authorization":format!("Bearer {}",cfg.reply_token.0),"Priority":"2","Content-Type":"text/plain"},
        "body":token.0,"clear":true
    })).collect();
    publish(
        cfg,
        json!({"topic":cfg.settings.questions,"title":notification.title,"message":notification.body,
        "priority":2,"sequence_id":notification.toast_id,"actions":actions}),
    )
}

fn publish(cfg: &Config, payload: Value) -> Result<(), String> {
    // No server response text, URL, headers or library error is copied into logs/health: an upstream
    // may echo a request, and request payloads contain answer and publish credentials.
    let mut response = agent()
        .post(&format!("{}/", cfg.settings.server))
        .header("Authorization", &format!("Bearer {}", cfg.daemon_token.0))
        .header("Content-Type", "application/json")
        .send(payload.to_string())
        .map_err(|_| "phone publication failed; it will retry".to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "ntfy refused publication (HTTP {}); check credentials and limits",
            response.status().as_u16()
        ));
    }
    let response = response
        .body_mut()
        .with_config()
        .limit(16_384)
        .read_to_string()
        .map_err(|_| "ntfy publication response was incomplete".to_string())?;
    let response: Value = serde_json::from_str(&response)
        .map_err(|_| "ntfy publication response was invalid".to_string())?;
    if response["event"] != "message"
        || response["topic"] != payload["topic"]
        || !response["id"].as_str().is_some_and(phone::identifier)
    {
        return Err("ntfy did not confirm publication; it will retry".into());
    }
    Ok(())
}

fn poll(cfg: &Config, cursor: Option<&str>) -> Result<(Vec<Incoming>, Option<String>), String> {
    let since = cursor
        .map(str::to_owned)
        .unwrap_or_else(|| cfg.since.to_string());
    let url = format!(
        "{}/{}/json?poll=1&since={since}",
        cfg.settings.server, cfg.settings.replies
    );
    let mut response = agent()
        .get(&url)
        .header("Authorization", &format!("Bearer {}", cfg.daemon_token.0))
        .call()
        .map_err(|_| "phone reply check failed; waiting to reconnect".to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "ntfy refused reply reads (HTTP {})",
            response.status().as_u16()
        ));
    }
    if response
        .headers()
        .get("X-Messages-Truncated")
        .is_some_and(|h| h == "1")
    {
        return Err("ntfy truncated the reply history; no checkpoint advanced. Check the Desk and reply topic.".into());
    }
    let body = response
        .body_mut()
        .with_config()
        .limit(2_097_152)
        .read_to_string()
        .map_err(|_| {
            "phone reply history exceeded the safe size or was interrupted; no checkpoint advanced"
                .to_string()
        })?;
    let mut messages = Vec::new();
    let mut last = None;
    let mut count = 0;
    for line in body.lines().filter(|s| !s.trim().is_empty()) {
        count += 1;
        if count > 500 {
            return Err("too many phone replies in one page; no checkpoint advanced".into());
        }
        let message: Value = serde_json::from_str(line)
            .map_err(|_| "invalid phone reply history; no checkpoint advanced".to_string())?;
        if message["event"] != "message" {
            continue;
        }
        if message["topic"].as_str() != Some(&cfg.settings.replies) {
            return Err("phone reply history contained an unexpected topic".into());
        }
        let id = message["id"]
            .as_str()
            .filter(|s| phone::identifier(s))
            .ok_or("phone reply history contained an invalid id")?;
        last = Some(id.to_owned());
        if message["time"].as_i64().is_none_or(|t| t < cfg.since) {
            continue;
        }
        if let Some(token) = message["message"].as_str().filter(|s| s.len() <= 128) {
            messages.push(Incoming {
                id: id.to_owned(),
                token: Secret(token.to_owned()),
            });
        }
    }
    Ok((messages, last))
}

async fn deliver(shared: &Arc<Shared>, id: &str) -> Result<(), String> {
    if !phone::identifier(id) {
        return Err("invalid phone receipt id".into());
    }
    let exe = agentbolt_ipc::sibling_paths(agentbolt_ipc::DESK_EXE)
        .into_iter()
        .find(|p| p.is_file())
        .ok_or("installed Desk is missing; phone reply remains queued")?;
    let mut command = tokio::process::Command::new(exe);
    command
        .args(["--channel", &shared.names.channel, "--phone-reply", id])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    let mut child = command
        .spawn()
        .map_err(|_| "could not start the Desk; phone reply remains queued".to_string())?;
    let status = tokio::time::timeout(Duration::from_secs(30), child.wait())
        .await
        .map_err(|_| "Desk reply timed out; the saved receipt will be retried".to_string())?
        .map_err(|_| {
            "Desk reply status unavailable; the saved receipt will be retried".to_string()
        })?;
    if !status.success() {
        return Err("Desk could not process the phone reply; it remains queued".into());
    }
    Ok(())
}
