//! PC test server for the PLC socket link (`docs/link/wire-spec.md`, usage `docs/link/README.md`).
//!
//! * [`server`]   hub + PLC port + passive PLC clients + control API
//! * [`hub`]      shared state: links, last messages, commands, log ring, events
//! * [`plc_port`] listener for active PLCs (framing sniffing, HTTP-active routes)
//! * [`session`]  hub driver of a `plc_link::io::LinkSession` (FRAME / NDJSON)
//! * [`client`]   `--connect` clients for passive PLCs
//! * [`api`]      control REST / SSE / HTML
//! * [`sim`]      PLC simulator (active and passive)
//! * [`selftest`] in-process matrix of server × simulator

pub mod api;
pub mod cli;
pub mod client;
pub mod contract;
pub mod httpc;
pub mod hub;
pub mod log;
pub mod plc_port;
pub mod selftest;
pub mod server;
pub mod session;
pub mod sim;
pub mod util;
pub mod wire;
