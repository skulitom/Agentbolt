//! agentbolt: the CLI and MCP server (a console program: PLAN.md, Anode B-050).

fn main() {
    std::process::exit(agentbolt::run(std::env::args().skip(1)));
}
