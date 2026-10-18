//! Agentbolt core: the data model, the SQLite store, the rules (budget, owners, dedupe, defaults), per-button
//! tokens, the event chain, and the pipe protocol. Pure Rust, no OS or GUI dependencies, so it builds and tests
//! on Linux (PLAN.md: the MCP front-end must start on Linux for Glama, Anode B-049).

pub mod channel;
pub mod model;
pub mod protocol;
pub mod service;
