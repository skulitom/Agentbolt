//! Agentbolt core: the data model, the SQLite store, the rules (budget, owners, dedupe, defaults), per-button
//! tokens, the event chain, and the pipe protocol. Pure Rust, no OS or GUI dependencies, so it builds and tests
//! on Linux (PLAN.md: the MCP front-end must start on Linux for Glama, Anode B-049).

pub mod board;
pub mod channel;
pub mod events;
pub mod holds;
pub mod metrics;
pub mod model;
pub mod navigation;
pub mod phone;
pub mod protocol;
pub mod rules;
pub mod service;
pub mod service_impl;
pub mod store;
pub mod tokens;

pub use service_impl::StoreService;

#[cfg(test)]
mod service_tests;
