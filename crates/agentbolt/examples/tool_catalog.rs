//! Regenerate the plugin's discovery fixture without a daemon, installation or MCP connection.
fn main() {
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"tools":agentbolt::tools::tools_for_host(600_000)})
        )
        .expect("tool catalog")
    );
}
