//! `/api/trace/*` — cycle-accurate PLC trace over the socket link.
//!
//! The PC picks the variables, the PLC samples them every cycle and pushes fixed-size chunks (wire-spec section 6).
//! [`channels`] turns the contract into the selectable variables and into the PEEK descriptors the PLC expects;
//! the chunks themselves are decoded by `plc_link::trace`.

pub mod channels;
pub mod routes;
pub mod store;

pub use store::TraceStore;
