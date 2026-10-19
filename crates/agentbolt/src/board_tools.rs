//! Compact Board discovery and argument conversion. The core owns all coordination rules.
use agentbolt_core::board::BoardOp;
use rmcp::model::{JsonObject, Tool};
use serde_json::{Value, json};

pub fn tools() -> Vec<Tool> {
    let handle = json!({"type":"string"});
    let id = json!({"type":"string"});
    let fence = json!({"type":"integer","minimum":1});
    vec![
        crate::tools::tool(
            "board_who",
            "Who is working",
            "See agents/roles/work; next as after. Inactive is opt-in.",
            json!({"type":"object","properties":{"handle":handle,"after":{"type":"integer","minimum":0},"include_inactive":{"type":"boolean"}}}),
            true,
        ),
        crate::tools::tool(
            "board_post",
            "Send a Board message",
            "Message role/owner/public agent ID/everyone. Thread/dedupe retries.",
            json!({"type":"object","properties":{
            "handle":handle,"title":{"type":"string"},"body":{"type":"string"},"references":{"type":"array","items":{"type":"string"}},
            "to":{"type":"object","properties":{"kind":{"enum":["everyone","role","agent","owner"]},"id":{"type":"string"}},"required":["kind"]},
            "thread":{"type":"string"},"dedupe_key":{"type":"string"}},"required":["title","body"]}),
            false,
        ),
        crate::tools::tool(
            "board_read",
            "Read Board messages",
            "Read routed/sent messages; nonconsuming. Page with next as after.",
            json!({"type":"object","properties":{"handle":handle,"after":{"type":"integer","minimum":0},"thread":{"type":"string"}}}),
            true,
        ),
        crate::tools::tool(
            "board_request",
            "Create or inspect a handoff",
            "Create/list/get/cancel handoffs; dedupe, ten open per owner.",
            json!({"type":"object","properties":{
            "handle":handle,"action":{"enum":["request","requests","get","cancel"]},"id":id,"include_closed":{"type":"boolean"},"after":{"type":"integer","minimum":0},
            "title":{"type":"string"},"body":{"type":"string"},"to_role":{"type":"string"},"repo":{"type":"string"},
            "paths":{"type":"array","items":{"type":"string"}},"dedupe_key":{"type":"string"},"beads_issue":{"type":"string"}}}),
            false,
        ),
        crate::tools::tool(
            "board_claim",
            "Claim or renew a handoff",
            "Claim; save fence. Handle calls renew live claims; fence for release.",
            json!({"type":"object","properties":{"handle":handle,"action":{"enum":["claim","renew","release"]},"id":id,"fence":fence},"required":["id"]}),
            false,
        ),
        crate::tools::tool(
            "board_done",
            "Finish your claimed handoff",
            "Finish with fence; failed:true reports failure.",
            json!({"type":"object","properties":{"handle":handle,"id":id,"fence":fence,"result":{"type":"string","description":"Record supplied results verbatim, preserving limits and qualifiers."},"failed":{"type":"boolean"}},"required":["id","fence","result"]}),
            false,
        ),
    ]
}

pub fn parse(name: &str, args: &JsonObject) -> Result<BoardOp, String> {
    let (default, allowed): (&str, &[&str]) = match name {
        "board_who" => ("who_page", &["who_page"]),
        "board_post" => ("post", &["post"]),
        "board_read" => ("read", &["read"]),
        "board_request" => ("request", &["request", "requests", "get", "cancel"]),
        "board_claim" => ("claim", &["claim", "renew", "release"]),
        "board_done" => ("done", &["done"]),
        _ => return Err("unknown Board tool".into()),
    };
    let mut args = args.clone();
    args.remove("handle");
    let action = args
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or(default);
    if !allowed.contains(&action) {
        return Err(format!("{name} accepts {}", allowed.join(", ")));
    }
    let action = action.to_owned();
    if matches!(action.as_str(), "post" | "request") {
        args.remove("action");
        return serde_json::from_value(json!({"action":action,"input":args}))
            .map_err(|e| format!("Board arguments: {e}"));
    }
    args.insert("action".into(), json!(action));
    serde_json::from_value(Value::Object(args)).map_err(|e| format!("Board arguments: {e}"))
}

pub fn profile(args: &JsonObject) -> Result<Option<BoardOp>, String> {
    if !args.contains_key("roles") && !args.contains_key("repo") && !args.contains_key("clear_repo")
    {
        return Ok(None);
    }
    let roles = args
        .get("roles")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|_| "roles must be a list of strings")?;
    let repo = crate::tools::str_arg(args, "repo")?;
    let clear_repo = args
        .get("clear_repo")
        .map(|v| v.as_bool().ok_or("clear_repo must be a boolean"))
        .transpose()?
        .unwrap_or(false);
    Ok(Some(BoardOp::Profile {
        roles,
        repo,
        clear_repo,
    }))
}

pub fn holds_tool() -> Tool {
    crate::tools::tool(
        "work_holds",
        "Check overlapping work",
        "Check PR/merge overlaps; unknown is not clear. Manage own holds.",
        json!({"type":"object","properties":{
        "handle":{"type":"string"},"action":{"enum":["list","check","acquire","release"]},"after":{"type":"string"},"repo":{"type":"string"},"paths":{"type":"array","items":{"type":"string"}},"stage":{"enum":["pr_open","merge"]},"note":{"type":"string"},"id":{"type":"string"}},"required":["action"]}),
        false,
    )
}
pub fn holds_args(args: &JsonObject) -> Result<agentbolt_core::holds::HoldOp, String> {
    let mut value = args.clone();
    value.remove("handle");
    if value.get("action").and_then(Value::as_str) == Some("list") {
        value.insert("action".into(), json!("list_page"));
    }
    serde_json::from_value(Value::Object(value)).map_err(|e| format!("hold arguments: {e}"))
}
