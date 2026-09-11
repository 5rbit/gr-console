//! Minimal S7comm (ISO-on-TCP, TCP 102) client.
//!
//! Scope: DB area byte reads/writes on S7-1200/1500 (PUT/GET permitted, standard-access DBs) and
//! S7-300/400. No native dependency. [`testing::FakeS7Server`] (feature `testing`, always on for this
//! crate's own tests) serves in-memory DBs for integration tests.

pub mod client;
pub mod cotp;
pub mod error;
pub mod pdu;
pub mod testing;

pub use client::{DbRead, DbWrite, ProbeResult, S7Client, S7Config};
pub use error::S7Error;
