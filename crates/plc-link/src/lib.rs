//! Sans-io PLC socket link (wire contract: `docs/link/wire-spec.md`).
//!
//! * [`registry`]  message registry (`plc/link/messages.toml`) resolved against a PLC contract
//! * [`header`]    16-byte FRAME header
//! * [`wire_json`] byte-exact JSON value text <-> standard-layout UDT bytes
//! * [`envelope`]  `{"type","seq","sig","data"}` envelope
//! * [`framing`]   incremental FRAME / NDJSON / HTTP decoders and encoders, framing sniffing
//! * [`codec`]     frames <-> canonical [`codec::Message`] (UDT bytes)
//! * [`vectors`]   deterministic golden values
//! * [`codegen`]   `gr-contract gen-link`: generated SCL, constant table, test vector DB, PC artifacts (`src/gen/`)

pub mod codec;
// `gen` is a reserved keyword in edition 2024
#[path = "gen/mod.rs"]
pub mod codegen;
pub mod envelope;
pub mod error;
pub mod framing;
pub mod header;
pub mod registry;
pub mod vectors;
pub mod wire_json;

pub use codec::{Codec, Message, Role};
pub use envelope::{Envelope, parse_envelope, write_envelope};
pub use error::{ErrCode, LinkError};
pub use framing::{Framing, HttpMeta, RawFrame};
pub use header::{Format, FrameHeader};
pub use registry::{Direction, MessageDef, MessageSpec, Registry, RegistryDoc};
pub use wire_json::{Report, Strictness};
